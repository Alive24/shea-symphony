use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs, io::Write};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::codex_app_server::{normalize_json_rpc_line, CodexAppServerEventKind};
use crate::config::RuntimeConfig;
use crate::model::AgentEvent;
use crate::profiles::{selected_execution_profile, ExecutionProfile};
use crate::session_registry::{
    deterministic_session_name, save_session_record, session_registry_path, unix_timestamp_ms,
    update_session_record_status, AgentSessionRecord, SessionStatus,
};

const DEFAULT_TMUX_CAPTURE_LINES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedRun {
    pub backend: String,
    pub workspace: PathBuf,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_artifact_path: Option<PathBuf>,
    pub command: Option<String>,
    pub timeout_ms: u64,
    #[serde(default)]
    pub stall_timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_sandbox_policy: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_server_resume_thread_id: Option<String>,
    pub profile_id: Option<String>,
    pub instance_name: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub actor_role: Option<String>,
    pub actor_label: Option<String>,
    pub git_author: Option<String>,
    pub issue_id: Option<String>,
    pub issue_identifier: Option<String>,
    pub issue_title: Option<String>,
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub attempt: u32,
    pub branch_name: Option<String>,
    pub session_registry_path: Option<PathBuf>,
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
            stall_timeout_ms: 0,
            model: None,
            reasoning_effort: None,
            approval_policy: None,
            sandbox: None,
            turn_sandbox_policy: None,
            app_server_resume_thread_id: None,
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            env: profile_environment(profile.as_ref(), self.name()),
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
            issue_id: None,
            issue_identifier: None,
            issue_title: None,
            lane: None,
            run_id: None,
            attempt: 1,
            branch_name: None,
            session_registry_path: None,
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
            stall_timeout_ms: config.codex.stall_timeout_ms,
            model: config.codex.model.clone(),
            reasoning_effort: Some(config.codex.reasoning_effort.clone()),
            approval_policy: Some(config.codex.approval_policy.to_string()),
            sandbox: Some(config.codex.thread_sandbox.clone()),
            turn_sandbox_policy: config.codex.turn_sandbox_policy.clone(),
            app_server_resume_thread_id: None,
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            env: profile_environment(profile.as_ref(), self.name()),
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
            issue_id: None,
            issue_identifier: None,
            issue_title: None,
            lane: None,
            run_id: None,
            attempt: 1,
            branch_name: None,
            session_registry_path: Some(session_registry_path(config)),
        })
    }

    fn run(&self, prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
        if prepared
            .command
            .as_deref()
            .is_some_and(is_codex_app_server_command)
        {
            return run_codex_app_server_backend(prepared);
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
            stall_timeout_ms: 0,
            model: None,
            reasoning_effort: None,
            approval_policy: None,
            sandbox: None,
            turn_sandbox_policy: None,
            app_server_resume_thread_id: None,
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            env: profile_environment(profile.as_ref(), self.name()),
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
            issue_id: None,
            issue_identifier: None,
            issue_title: None,
            lane: None,
            run_id: None,
            attempt: 1,
            branch_name: None,
            session_registry_path: Some(session_registry_path(config)),
        })
    }

    fn run(&self, prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
        run_claude_stream_json_backend(prepared)
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
            "SHEA_SYMPHONY_TMUX_COMMAND".into(),
            config.tmux.command.clone(),
        );
        env.insert(
            "SHEA_SYMPHONY_TMUX_SESSION_PREFIX".into(),
            config.tmux.session_prefix.clone(),
        );
        env.insert(
            "SHEA_SYMPHONY_WORKSPACE_ROOT".into(),
            config.workspace.root.display().to_string(),
        );
        Ok(PreparedRun {
            backend: self.name().into(),
            workspace,
            prompt: rendered_prompt,
            prompt_artifact_path: None,
            command: Some(config.tmux.agent_command.clone()),
            timeout_ms: 0,
            stall_timeout_ms: 0,
            model: None,
            reasoning_effort: None,
            approval_policy: None,
            sandbox: None,
            turn_sandbox_policy: None,
            app_server_resume_thread_id: None,
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            env,
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
            issue_id: None,
            issue_identifier: None,
            issue_title: None,
            lane: None,
            run_id: None,
            attempt: 1,
            branch_name: None,
            session_registry_path: Some(session_registry_path(config)),
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
        .env("SHEA_SYMPHONY_PROMPT_PATH", &prompt_artifact_path)
        .env(
            "SHEA_SYMPHONY_APPROVAL_POLICY",
            prepared.approval_policy.as_deref().unwrap_or_default(),
        )
        .env(
            "SHEA_SYMPHONY_SANDBOX",
            prepared.sandbox.as_deref().unwrap_or_default(),
        )
        .envs(prepared.env.iter())
        .env(
            "SHEA_SYMPHONY_ACTOR_ROLE",
            prepared.actor_role.as_deref().unwrap_or_default(),
        )
        .env(
            "SHEA_SYMPHONY_ACTOR_LABEL",
            prepared.actor_label.as_deref().unwrap_or_default(),
        )
        .env(
            "SHEA_SYMPHONY_GIT_AUTHOR",
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

const CLAUDE_STREAM_JSON_SOURCE: &str = "claude-code-stream-json";
const CLAUDE_RESUME_SESSION_ENV: &str = "SHEA_SYMPHONY_CLAUDE_RESUME_SESSION_ID";

#[derive(Debug, Clone)]
struct ClaudeArtifactPaths {
    protocol_path: PathBuf,
    stderr_path: PathBuf,
    events_path: PathBuf,
}

#[derive(Default)]
struct ClaudeProtocolState {
    session_id: Option<String>,
    initialized: bool,
    terminal: bool,
}

fn run_claude_stream_json_backend(prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
    run_claude_stream_json_backend_with_cancel(prepared, None)
}

/// Runs the shared Claude stream-json transport for one independently owned
/// Review job. Cancellation remains an adapter concern; parsing, process
/// supervision, timeout handling, and artifacts stay in the canonical transport.
pub(crate) fn run_claude_stream_json_review(
    prepared: PreparedRun,
    cancel: &AtomicBool,
) -> Result<Vec<AgentEvent>, AgentError> {
    run_claude_stream_json_backend_with_cancel(prepared, Some(cancel))
}

/// Binds a resumed Claude session to the current prepared run. Callers must
/// validate issue, lane, run, and workspace ownership before using this helper.
pub(crate) fn set_claude_resume_session(prepared: &mut PreparedRun, session_id: String) {
    prepared
        .env
        .insert(CLAUDE_RESUME_SESSION_ENV.into(), session_id);
}

fn run_claude_stream_json_backend_with_cancel(
    prepared: PreparedRun,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<AgentEvent>, AgentError> {
    let mut events = Vec::new();
    if !prepared.workspace.is_dir() {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error: format!("workspace does not exist: {}", prepared.workspace.display()),
        });
        return Ok(events);
    }
    let Some(base_command) = prepared.command.as_deref() else {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error: "missing Claude Code command".into(),
        });
        return Ok(events);
    };

    let prompt_artifact_path = persist_prompt_artifact(&prepared)?;
    let artifacts = claude_artifact_paths(&prepared, &prompt_artifact_path);
    if let Some(parent) = artifacts.protocol_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let invocation = claude_stream_json_command(
        base_command,
        prepared
            .env
            .get(CLAUDE_RESUME_SESSION_ENV)
            .map(String::as_str),
    );
    let mut command = Command::new("sh");
    command
        .arg("-lc")
        .arg(&invocation)
        .current_dir(&prepared.workspace)
        .env("SHEA_SYMPHONY_PROMPT_PATH", &prompt_artifact_path)
        .envs(prepared.env.iter())
        .env(
            "SHEA_SYMPHONY_ACTOR_ROLE",
            prepared.actor_role.as_deref().unwrap_or_default(),
        )
        .env(
            "SHEA_SYMPHONY_ACTOR_LABEL",
            prepared.actor_label.as_deref().unwrap_or_default(),
        )
        .env(
            "SHEA_SYMPHONY_GIT_AUTHOR",
            prepared.git_author.as_deref().unwrap_or_default(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_child_process_group(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            fs::write(&artifacts.protocol_path, "")?;
            fs::write(&artifacts.stderr_path, error.to_string())?;
            events.push(AgentEvent::Failed {
                backend: prepared.backend.clone(),
                error: format!("Claude Code startup failed: {error}"),
            });
            finish_claude_artifacts(
                &prepared,
                &prompt_artifact_path,
                &artifacts,
                &mut events,
                None,
            )?;
            return Ok(events);
        }
    };

    let process_id = child.id();
    let Some(mut stdin) = child.stdin.take() else {
        terminate_child_process_group(&mut child);
        events.push(AgentEvent::Failed {
            backend: prepared.backend.clone(),
            error: "Claude Code stdin was unavailable".into(),
        });
        finish_claude_artifacts(
            &prepared,
            &prompt_artifact_path,
            &artifacts,
            &mut events,
            None,
        )?;
        return Ok(events);
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AgentError::Unavailable("Claude Code stdout was unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AgentError::Unavailable("Claude Code stderr was unavailable".into()))?;
    let stdout_rx = spawn_app_server_stdout_reader(stdout);
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut stderr_text = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut stderr_text);
        let _ = stderr_tx.send(stderr_text);
    });

    let input = serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": prepared.prompt}]
        },
        "parent_tool_use_id": null
    });
    let input_line = serde_json::to_string(&input)
        .map_err(|error| AgentError::Unavailable(error.to_string()))?;
    let mut protocol_log = String::new();
    append_protocol_record(&mut protocol_log, "stdin", &input_line);
    if let Err(error) = writeln!(stdin, "{input_line}").and_then(|_| stdin.flush()) {
        terminate_child_process_group(&mut child);
        events.push(AgentEvent::Failed {
            backend: prepared.backend.clone(),
            error: format!("Claude Code input write failed: {error}"),
        });
    }
    drop(stdin);

    let deadline = Instant::now() + Duration::from_millis(prepared.timeout_ms.max(1));
    let mut state = ClaudeProtocolState::default();
    let mut force_stop = events
        .iter()
        .any(|event| matches!(event, AgentEvent::Failed { .. }));
    while !force_stop {
        match next_claude_stdout_line(&mut child, &stdout_rx, deadline, cancel) {
            Ok(Some(line)) => {
                append_protocol_record(&mut protocol_log, "stdout", &line);
                if let Err(error) = normalize_claude_stream_line(
                    &prepared,
                    &artifacts,
                    &prompt_artifact_path,
                    process_id,
                    &line,
                    &mut state,
                    &mut events,
                ) {
                    events.push(AgentEvent::Failed {
                        backend: prepared.backend.clone(),
                        error,
                    });
                    force_stop = true;
                }
            }
            Ok(None) => break,
            Err(error) => {
                events.push(AgentEvent::Failed {
                    backend: prepared.backend.clone(),
                    error,
                });
                force_stop = true;
            }
        }
    }

    let exit_status = if force_stop {
        terminate_child_process_group(&mut child)
    } else {
        child.wait().ok()
    };
    if !state.terminal
        && !events
            .iter()
            .any(|event| matches!(event, AgentEvent::Failed { .. }))
    {
        events.push(AgentEvent::Failed {
            backend: prepared.backend.clone(),
            error: if !state.initialized
                && exit_status.as_ref().is_some_and(|status| !status.success())
            {
                format!(
                    "Claude Code startup failed with status {} before initialization",
                    display_exit_status(exit_status.as_ref())
                )
            } else {
                "Claude Code stream ended before a terminal result event".into()
            },
        });
    }
    if exit_status.as_ref().is_some_and(|status| !status.success())
        && !events.iter().any(|event| {
            matches!(event, AgentEvent::Failed { error, .. } if error.contains("timed out"))
        })
    {
        events.push(AgentEvent::Failed {
            backend: prepared.backend.clone(),
            error: format!(
                "Claude Code exited with status {}",
                display_exit_status(exit_status.as_ref())
            ),
        });
    }

    fs::write(&artifacts.protocol_path, &protocol_log)?;
    let stderr_text = stderr_rx
        .recv_timeout(Duration::from_millis(500))
        .unwrap_or_default();
    fs::write(&artifacts.stderr_path, &stderr_text)?;
    if !stderr_text.trim().is_empty() {
        events.push(AgentEvent::Message {
            backend: prepared.backend.clone(),
            session_id: state.session_id.clone(),
            text: stderr_text,
        });
    }
    if let Some(session_id) = state.session_id.as_deref() {
        update_claude_session_status(
            prepared.session_registry_path.as_deref(),
            session_id,
            if events
                .iter()
                .any(|event| matches!(event, AgentEvent::Completed { .. }))
                && !events
                    .iter()
                    .any(|event| matches!(event, AgentEvent::Failed { .. }))
            {
                SessionStatus::Completed
            } else if usage_limit_pause_from_events(&events).is_some() {
                SessionStatus::UsageLimited
            } else {
                SessionStatus::Failed
            },
        )?;
    }
    finish_claude_artifacts(
        &prepared,
        &prompt_artifact_path,
        &artifacts,
        &mut events,
        exit_status.as_ref(),
    )?;
    Ok(events)
}

/// Protocol arguments appended to every configured Claude Code command.
pub(crate) fn claude_stream_json_args() -> Vec<String> {
    [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn claude_stream_json_command(base_command: &str, resume_session_id: Option<&str>) -> String {
    let mut command = format!("exec {}", base_command.trim());
    for argument in claude_stream_json_args() {
        command.push(' ');
        command.push_str(&argument);
    }
    if let Some(session_id) = resume_session_id.filter(|value| !value.trim().is_empty()) {
        command.push_str(" --resume ");
        command.push_str(&shell_quote_str(session_id));
    }
    command
}

fn configure_child_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn terminate_child_process_group(child: &mut Child) -> Option<ExitStatus> {
    if let Ok(Some(status)) = child.try_wait() {
        return Some(status);
    }
    #[cfg(unix)]
    {
        // The child is its process-group leader, so a negative PID reaches wrappers and
        // descendants together. A stale descendant must never outlive a failed lane run.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGTERM);
        }
        for _ in 0..10 {
            if let Ok(Some(status)) = child.try_wait() {
                return Some(status);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    let _ = child.kill();
    child.wait().ok()
}

fn next_claude_stdout_line(
    child: &mut Child,
    stdout_rx: &Receiver<Result<String, String>>,
    deadline: Instant,
    cancel: Option<&AtomicBool>,
) -> Result<Option<String>, String> {
    loop {
        if cancel.is_some_and(|cancel| cancel.load(AtomicOrdering::Relaxed)) {
            terminate_child_process_group(child);
            return Err(
                "Claude Code execution was cancelled before a terminal result event".into(),
            );
        }
        let now = Instant::now();
        if now >= deadline {
            terminate_child_process_group(child);
            return Err("Claude Code timed out waiting for a terminal result event".into());
        }
        match stdout_rx.recv_timeout(
            deadline
                .saturating_duration_since(now)
                .min(Duration::from_millis(50)),
        ) {
            Ok(Ok(line)) => return Ok(Some(line)),
            Ok(Err(error)) => return Err(format!("Claude Code stdout read failed: {error}")),
            Err(RecvTimeoutError::Timeout) => {
                if child
                    .try_wait()
                    .map_err(|error| error.to_string())?
                    .is_some()
                {
                    return Ok(None);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return Ok(None),
        }
    }
}

fn normalize_claude_stream_line(
    prepared: &PreparedRun,
    artifacts: &ClaudeArtifactPaths,
    prompt_artifact_path: &Path,
    process_id: u32,
    line: &str,
    state: &mut ClaudeProtocolState,
    events: &mut Vec<AgentEvent>,
) -> Result<(), String> {
    // Claude may add progress event kinds over time. Preserve unknown non-terminal
    // kinds as evidence, but accept success only from the explicit final result.
    if state.terminal {
        return Err("Claude Code emitted data after its terminal result event".into());
    }
    let value: serde_json::Value = serde_json::from_str(line.trim())
        .map_err(|error| format!("Claude Code emitted malformed stream-json: {error}"))?;
    let event_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            "Claude Code stream-json event is missing string field `type`".to_string()
        })?;
    validate_claude_event_session(state, &value)?;

    match event_type {
        "system" if value.get("subtype").and_then(serde_json::Value::as_str) == Some("init") => {
            if state.initialized {
                return Err("Claude Code emitted more than one initialization event".into());
            }
            let session_id = value
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "Claude Code initialization is missing session_id".to_string())?
                .to_string();
            state.initialized = true;
            state.session_id = Some(session_id.clone());
            events.push(AgentEvent::SessionStarted {
                backend: prepared.backend.clone(),
                session_id: session_id.clone(),
            });
            save_claude_session_record(
                prepared,
                artifacts,
                prompt_artifact_path,
                &session_id,
                process_id,
            )
            .map_err(|error| error.to_string())?;
            events.push(AgentEvent::Message {
                backend: prepared.backend.clone(),
                session_id: Some(session_id),
                text: "claude_event type=system subtype=init".into(),
            });
        }
        "assistant" => {
            require_claude_initialization(state, event_type)?;
            normalize_claude_assistant(prepared, &value, state, events);
        }
        "user" => {
            require_claude_initialization(state, event_type)?;
            normalize_claude_tool_results(prepared, &value, state, events);
        }
        "result" => {
            require_claude_initialization(state, event_type)?;
            state.terminal = true;
            let subtype = value
                .get("subtype")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let is_error = value
                .get("is_error")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(subtype != "success");
            if is_error || subtype != "success" {
                let detail = value
                    .get("result")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(subtype);
                events.push(AgentEvent::Failed {
                    backend: prepared.backend.clone(),
                    error: format!("Claude Code result failed ({subtype}): {detail}"),
                });
            } else {
                let summary = value
                    .get("result")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("Claude Code completed successfully.")
                    .to_string();
                events.push(AgentEvent::Completed {
                    backend: prepared.backend.clone(),
                    session_id: state.session_id.clone(),
                    summary,
                });
            }
        }
        "error" => {
            state.terminal = true;
            events.push(AgentEvent::Failed {
                backend: prepared.backend.clone(),
                error: format!(
                    "Claude Code error event: {}",
                    value
                        .get("error")
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| value.get("message").and_then(serde_json::Value::as_str))
                        .unwrap_or("unspecified error")
                ),
            });
        }
        _ => {
            events.push(AgentEvent::Message {
                backend: prepared.backend.clone(),
                session_id: state.session_id.clone(),
                text: format!(
                    "claude_event type={event_type} subtype={}",
                    value
                        .get("subtype")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("n/a")
                ),
            });
        }
    }
    Ok(())
}

fn validate_claude_event_session(
    state: &ClaudeProtocolState,
    value: &serde_json::Value,
) -> Result<(), String> {
    let Some(actual) = value
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(());
    };
    if let Some(expected) = state.session_id.as_deref() {
        if actual != expected {
            return Err(format!(
                "Claude Code session changed within one stream: expected {expected}, got {actual}"
            ));
        }
    }
    Ok(())
}

fn require_claude_initialization(
    state: &ClaudeProtocolState,
    event_type: &str,
) -> Result<(), String> {
    if state.initialized {
        Ok(())
    } else {
        Err(format!(
            "Claude Code emitted {event_type} before initialization"
        ))
    }
}

fn normalize_claude_assistant(
    prepared: &PreparedRun,
    value: &serde_json::Value,
    state: &ClaudeProtocolState,
    events: &mut Vec<AgentEvent>,
) {
    if let Some(content) = value
        .pointer("/message/content")
        .and_then(serde_json::Value::as_array)
    {
        for block in content {
            match block.get("type").and_then(serde_json::Value::as_str) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(serde_json::Value::as_str) {
                        events.push(AgentEvent::Message {
                            backend: prepared.backend.clone(),
                            session_id: state.session_id.clone(),
                            text: text.to_string(),
                        });
                    }
                }
                Some("tool_use") => events.push(AgentEvent::Message {
                    backend: prepared.backend.clone(),
                    session_id: state.session_id.clone(),
                    text: format!(
                        "claude_tool_use name={} id={}",
                        block
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown"),
                        block
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown")
                    ),
                }),
                Some(kind) => events.push(AgentEvent::Message {
                    backend: prepared.backend.clone(),
                    session_id: state.session_id.clone(),
                    text: format!("claude_assistant_block type={kind}"),
                }),
                None => {}
            }
        }
    }
    if let Some(usage) = value.pointer("/message/usage") {
        let input_tokens = json_u64(usage.get("input_tokens"));
        let output_tokens = json_u64(usage.get("output_tokens"));
        if input_tokens > 0 || output_tokens > 0 {
            events.push(AgentEvent::TokenUsage {
                backend: prepared.backend.clone(),
                input_tokens,
                output_tokens,
                total_tokens: input_tokens.saturating_add(output_tokens),
            });
        }
    }
}

fn normalize_claude_tool_results(
    prepared: &PreparedRun,
    value: &serde_json::Value,
    state: &ClaudeProtocolState,
    events: &mut Vec<AgentEvent>,
) {
    let Some(content) = value
        .pointer("/message/content")
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    for block in content {
        if block.get("type").and_then(serde_json::Value::as_str) == Some("tool_result") {
            events.push(AgentEvent::Message {
                backend: prepared.backend.clone(),
                session_id: state.session_id.clone(),
                text: format!(
                    "claude_tool_result tool_use_id={} is_error={}",
                    block
                        .get("tool_use_id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown"),
                    block
                        .get("is_error")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                ),
            });
        }
    }
}

fn json_u64(value: Option<&serde_json::Value>) -> u64 {
    value
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}

fn claude_artifact_paths(
    prepared: &PreparedRun,
    prompt_artifact_path: &Path,
) -> ClaudeArtifactPaths {
    let artifact_dir = prompt_artifact_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("/tmp"))
        .join("claude-stream-json");
    let run_identity = prepared
        .run_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| safe_path_component(Some(value)))
        .unwrap_or_else(|| current_time_ms().to_string());
    let base = format!(
        "{}-{}-attempt-{}",
        safe_path_component(prepared.issue_identifier.as_deref()),
        run_identity,
        prepared.attempt.max(1)
    );
    ClaudeArtifactPaths {
        protocol_path: artifact_dir.join(format!("{base}.protocol.jsonl")),
        stderr_path: artifact_dir.join(format!("{base}.stderr.log")),
        events_path: artifact_dir.join(format!("{base}.events.json")),
    }
}

fn save_claude_session_record(
    prepared: &PreparedRun,
    artifacts: &ClaudeArtifactPaths,
    prompt_artifact_path: &Path,
    session_id: &str,
    process_id: u32,
) -> Result<(), AgentError> {
    let Some(registry_path) = prepared.session_registry_path.as_deref() else {
        return Ok(());
    };
    let now_ms = unix_timestamp_ms();
    save_session_record(
        registry_path,
        AgentSessionRecord {
            issue_id: prepared.issue_id.clone(),
            issue_identifier: prepared.issue_identifier.clone(),
            issue_title: prepared.issue_title.clone(),
            lane: prepared.lane.clone().unwrap_or_else(|| "main".into()),
            run_id: prepared.run_id.clone(),
            thread: Some(session_id.to_string()),
            session_source: Some(CLAUDE_STREAM_JSON_SOURCE.into()),
            claim_value: prepared.env.get("SHEA_SYMPHONY_CLAIM").cloned(),
            actor_role: prepared.actor_role.clone(),
            actor_label: prepared.actor_label.clone(),
            git_author: prepared.git_author.clone(),
            profile_id: prepared.profile_id.clone(),
            instance_name: prepared.instance_name.clone(),
            worktree: prepared.workspace.clone(),
            branch: prepared.branch_name.clone(),
            backend: prepared.backend.clone(),
            session_name: session_id.to_string(),
            process_id: Some(process_id),
            pane_target: String::new(),
            prompt_artifact_path: prompt_artifact_path.to_path_buf(),
            log_path: artifacts.events_path.clone(),
            attach_command: format!(
                "not an interactive session; inspect Claude artifacts: protocol={} stderr={} events={}",
                artifacts.protocol_path.display(),
                artifacts.stderr_path.display(),
                artifacts.events_path.display()
            ),
            attempt: prepared.attempt.max(1),
            status: SessionStatus::Running,
            started_at_ms: now_ms,
            updated_at_ms: now_ms,
        },
    )
    .map_err(|error| AgentError::Unavailable(format!("Claude session registry failed: {error}")))
}

fn update_claude_session_status(
    registry_path: Option<&Path>,
    session_id: &str,
    status: SessionStatus,
) -> Result<(), AgentError> {
    let Some(registry_path) = registry_path else {
        return Ok(());
    };
    update_session_record_status(registry_path, session_id, status, unix_timestamp_ms()).map_err(
        |error| AgentError::Unavailable(format!("Claude session registry failed: {error}")),
    )
}

fn finish_claude_artifacts(
    prepared: &PreparedRun,
    prompt_artifact_path: &Path,
    artifacts: &ClaudeArtifactPaths,
    events: &mut Vec<AgentEvent>,
    exit_status: Option<&ExitStatus>,
) -> Result<(), AgentError> {
    events.push(AgentEvent::Message {
        backend: prepared.backend.clone(),
        session_id: events.iter().find_map(|event| match event {
            AgentEvent::SessionStarted { session_id, .. } => Some(session_id.clone()),
            _ => None,
        }),
        text: format!(
            "claude_stream_json_artifacts prompt_artifact={} protocol_artifact={} stderr_artifact={} normalized_events_artifact={} exit_status={}",
            prompt_artifact_path.display(),
            artifacts.protocol_path.display(),
            artifacts.stderr_path.display(),
            artifacts.events_path.display(),
            display_exit_status(exit_status)
        ),
    });
    fs::write(
        &artifacts.events_path,
        serde_json::to_string_pretty(events)
            .map_err(|error| AgentError::Unavailable(error.to_string()))?,
    )?;
    Ok(())
}

fn run_codex_app_server_backend(prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
    run_codex_app_server_backend_with_options(prepared, AppServerRunOptions::default())
}

pub(crate) fn run_codex_app_server_review(
    prepared: PreparedRun,
    output_schema: &serde_json::Value,
    cancel: &AtomicBool,
) -> Result<Vec<AgentEvent>, AgentError> {
    run_codex_app_server_backend_with_options(
        prepared,
        AppServerRunOptions {
            output_schema: Some(output_schema),
            cancel: Some(cancel),
        },
    )
}

#[derive(Clone, Copy, Default)]
struct AppServerRunOptions<'a> {
    output_schema: Option<&'a serde_json::Value>,
    cancel: Option<&'a AtomicBool>,
}

#[derive(Clone, Copy)]
struct AppServerWaitOptions<'a> {
    timeout: Duration,
    cancel: Option<&'a AtomicBool>,
}

impl<'a> AppServerWaitOptions<'a> {
    fn new(timeout: Duration, cancel: Option<&'a AtomicBool>) -> Self {
        Self { timeout, cancel }
    }
}

fn run_codex_app_server_backend_with_options(
    prepared: PreparedRun,
    options: AppServerRunOptions<'_>,
) -> Result<Vec<AgentEvent>, AgentError> {
    let mut events = Vec::new();

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
            error: "missing Codex app-server command".into(),
        });
        return Ok(events);
    };

    let prompt_artifact_path = persist_prompt_artifact(&prepared)?;
    let artifacts = app_server_artifact_paths(&prepared, &prompt_artifact_path);
    if let Some(parent) = artifacts.protocol_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut protocol_log = String::new();
    let mut child = Command::new("sh")
        .arg("-lc")
        .arg(command)
        .current_dir(&prepared.workspace)
        .env("SHEA_SYMPHONY_PROMPT_PATH", &prompt_artifact_path)
        .env(
            "SHEA_SYMPHONY_APPROVAL_POLICY",
            prepared.approval_policy.as_deref().unwrap_or_default(),
        )
        .env(
            "SHEA_SYMPHONY_SANDBOX",
            prepared.sandbox.as_deref().unwrap_or_default(),
        )
        .envs(prepared.env.iter())
        .env(
            "SHEA_SYMPHONY_ACTOR_ROLE",
            prepared.actor_role.as_deref().unwrap_or_default(),
        )
        .env(
            "SHEA_SYMPHONY_ACTOR_LABEL",
            prepared.actor_label.as_deref().unwrap_or_default(),
        )
        .env(
            "SHEA_SYMPHONY_GIT_AUTHOR",
            prepared.git_author.as_deref().unwrap_or_default(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let Some(mut stdin) = child.stdin.take() else {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error: "Codex app-server stdin was unavailable".into(),
        });
        finish_app_server_run(child, None, None, &artifacts, &protocol_log)?;
        return Ok(events);
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AgentError::Unavailable("Codex app-server stdout was unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AgentError::Unavailable("Codex app-server stderr was unavailable".into()))?;
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut stderr_text = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut stderr_text);
        let _ = stderr_tx.send(stderr_text);
    });
    let stdout_rx = spawn_app_server_stdout_reader(stdout);

    let run_result = run_codex_app_server_protocol(
        &prepared,
        &mut stdin,
        &stdout_rx,
        &mut child,
        AppServerRegistryContext {
            artifacts: &artifacts,
            prompt_artifact_path: &prompt_artifact_path,
            options,
        },
        &mut protocol_log,
        &mut events,
    );

    drop(stdin);
    let terminal_session_id = events.iter().find_map(|event| match event {
        AgentEvent::SessionStarted { session_id, .. } => Some(session_id.clone()),
        _ => None,
    });
    let exit_status = finish_app_server_run(
        child,
        Some(stderr_rx),
        Some(&mut events),
        &artifacts,
        &protocol_log,
    )?;

    if let Err(error) = run_result {
        events.push(AgentEvent::Failed {
            backend: prepared.backend.clone(),
            error,
        });
    }
    if exit_status
        .as_ref()
        .and_then(ExitStatus::code)
        .is_some_and(|code| code != 0)
        && !events
            .iter()
            .any(|event| matches!(event, AgentEvent::Failed { .. }))
    {
        events.push(AgentEvent::Failed {
            backend: prepared.backend.clone(),
            error: format!(
                "Codex app-server exited unexpectedly with status {}",
                display_exit_status(exit_status.as_ref())
            ),
        });
    }
    if let Some(session_id) = terminal_session_id.as_deref() {
        update_app_server_session_status(
            prepared.session_registry_path.as_deref(),
            session_id,
            app_server_final_session_status(&events),
        )?;
    }

    events.push(AgentEvent::Message {
        backend: crate::codex_app_server::BACKEND_NAME.into(),
        session_id: terminal_session_id,
        text: format!(
            "app_server_artifacts prompt_artifact={} protocol_artifact={} stderr_artifact={} normalized_events_artifact={} exit_status={}",
            prompt_artifact_path.display(),
            artifacts.protocol_path.display(),
            artifacts.stderr_path.display(),
            artifacts.events_path.display(),
            display_exit_status(exit_status.as_ref())
        ),
    });
    fs::write(
        &artifacts.events_path,
        serde_json::to_string_pretty(&events)
            .map_err(|error| AgentError::Unavailable(error.to_string()))?,
    )?;

    Ok(events)
}

fn run_codex_app_server_protocol(
    prepared: &PreparedRun,
    stdin: &mut std::process::ChildStdin,
    stdout_rx: &Receiver<Result<String, String>>,
    child: &mut Child,
    registry_context: AppServerRegistryContext<'_>,
    protocol_log: &mut String,
    events: &mut Vec<AgentEvent>,
) -> Result<(), String> {
    let timeout = Duration::from_millis(prepared.timeout_ms.max(1));
    send_app_server_message(
        stdin,
        &serde_json::json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "capabilities": {
                    "experimentalApi": true
                },
                "clientInfo": {
                    "name": "shea-symphony",
                    "title": "Shea Symphony",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }),
        protocol_log,
    )?;
    await_app_server_response(
        child,
        stdout_rx,
        protocol_log,
        events,
        1,
        None,
        AppServerWaitOptions::new(timeout, registry_context.options.cancel),
    )?;

    send_app_server_message(
        stdin,
        &serde_json::json!({
            "method": "initialized",
            "params": {}
        }),
        protocol_log,
    )?;

    let thread_id = match prepared
        .app_server_resume_thread_id
        .as_deref()
        .filter(|thread_id| !thread_id.trim().is_empty())
    {
        Some(thread_id) => {
            let mut thread_params = serde_json::json!({
                "threadId": thread_id,
                "approvalPolicy": app_server_approval_policy(prepared.approval_policy.as_deref()),
                "cwd": prepared.workspace.display().to_string(),
                "sandbox": prepared.sandbox.as_deref().unwrap_or("workspace-write")
            });
            if let Some(model) = prepared
                .model
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                thread_params["model"] = serde_json::Value::String(model.to_string());
            }
            if let Some(reasoning_effort) = prepared
                .reasoning_effort
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                thread_params["reasoningEffort"] =
                    serde_json::Value::String(reasoning_effort.to_string());
            }

            let thread_resume = serde_json::json!({
                "method": "thread/resume",
                "id": 2,
                "params": thread_params
            });
            send_app_server_message(stdin, &thread_resume, protocol_log)?;
            let thread_response = await_app_server_response(
                child,
                stdout_rx,
                protocol_log,
                events,
                2,
                None,
                AppServerWaitOptions::new(timeout, registry_context.options.cancel),
            )?;
            let resumed_thread_id = thread_response
                .pointer("/result/thread/id")
                .or_else(|| thread_response.pointer("/thread/id"))
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    format!(
                        "Codex app-server thread/resume response missing thread id: {thread_response}"
                    )
                })?;
            if resumed_thread_id != thread_id {
                return Err(format!(
                    "Codex app-server thread/resume returned `{resumed_thread_id}` for requested thread `{thread_id}`"
                ));
            }
            resumed_thread_id.to_string()
        }
        None => {
            let mut thread_params = serde_json::json!({
                "approvalPolicy": app_server_approval_policy(prepared.approval_policy.as_deref()),
                "sandbox": prepared.sandbox.as_deref().unwrap_or("workspace-write"),
                "cwd": prepared.workspace.display().to_string(),
                "dynamicTools": []
            });
            if let Some(model) = prepared
                .model
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                thread_params["model"] = serde_json::Value::String(model.to_string());
            }
            if let Some(reasoning_effort) = prepared
                .reasoning_effort
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                thread_params["reasoningEffort"] =
                    serde_json::Value::String(reasoning_effort.to_string());
            }

            let thread_start = serde_json::json!({
                "method": "thread/start",
                "id": 2,
                "params": thread_params
            });
            send_app_server_message(stdin, &thread_start, protocol_log)?;
            let thread_response = await_app_server_response(
                child,
                stdout_rx,
                protocol_log,
                events,
                2,
                None,
                AppServerWaitOptions::new(timeout, registry_context.options.cancel),
            )?;
            thread_response
                .pointer("/result/thread/id")
                .or_else(|| thread_response.pointer("/thread/id"))
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    format!(
                        "Codex app-server thread/start response missing thread id: {thread_response}"
                    )
                })?
                .to_string()
        }
    };

    let mut turn_params = serde_json::json!({
        "threadId": thread_id.clone(),
        "input": [
            {
                "type": "text",
                "text": prepared.prompt.clone()
            }
        ],
        "cwd": prepared.workspace.display().to_string(),
        "title": app_server_turn_title(prepared),
        "approvalPolicy": app_server_approval_policy(prepared.approval_policy.as_deref())
    });
    if let Some(model) = prepared
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        turn_params["model"] = serde_json::Value::String(model.to_string());
    }
    if let Some(reasoning_effort) = prepared
        .reasoning_effort
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        turn_params["reasoningEffort"] = serde_json::Value::String(reasoning_effort.to_string());
    }
    if let Some(policy) = prepared.turn_sandbox_policy.clone() {
        turn_params["sandboxPolicy"] = policy;
    }
    if let Some(output_schema) = registry_context.options.output_schema {
        turn_params["outputSchema"] = output_schema.clone();
    }
    let turn_request_id = 3;
    send_app_server_message(
        stdin,
        &serde_json::json!({
            "method": "turn/start",
            "id": turn_request_id,
            "params": turn_params
        }),
        protocol_log,
    )?;
    let turn_response = await_app_server_response(
        child,
        stdout_rx,
        protocol_log,
        events,
        turn_request_id,
        None,
        AppServerWaitOptions::new(timeout, registry_context.options.cancel),
    )?;
    let turn_id = turn_response
        .pointer("/result/turn/id")
        .or_else(|| turn_response.pointer("/turn/id"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!("Codex app-server turn/start response missing turn id: {turn_response}")
        })?
        .to_string();
    let session_id = format!("{thread_id}-{turn_id}");
    events.push(AgentEvent::Message {
        backend: crate::codex_app_server::BACKEND_NAME.into(),
        session_id: Some(session_id.clone()),
        text: format!("app_server_identity thread_id={thread_id} turn_id={turn_id}"),
    });
    events.push(AgentEvent::SessionStarted {
        backend: prepared.backend.clone(),
        session_id: session_id.clone(),
    });
    save_app_server_session_record(
        prepared,
        registry_context.artifacts,
        registry_context.prompt_artifact_path,
        &thread_id,
        &session_id,
        child.id(),
        SessionStatus::Running,
    )
    .map_err(|error| error.to_string())?;

    let stall_timeout = if prepared.stall_timeout_ms == 0 {
        timeout
    } else {
        Duration::from_millis(prepared.stall_timeout_ms)
    };
    await_app_server_terminal_event(
        child,
        stdout_rx,
        protocol_log,
        events,
        &session_id,
        stall_timeout,
        AppServerWaitOptions::new(timeout, registry_context.options.cancel),
    )
}

fn send_app_server_message(
    stdin: &mut std::process::ChildStdin,
    payload: &serde_json::Value,
    protocol_log: &mut String,
) -> Result<(), String> {
    let line = serde_json::to_string(payload).map_err(|error| error.to_string())?;
    stdin
        .write_all(line.as_bytes())
        .and_then(|_| stdin.write_all(b"\n"))
        .and_then(|_| stdin.flush())
        .map_err(|error| format!("Codex app-server stdin write failed: {error}"))?;
    append_protocol_record(protocol_log, "stdin", &line);
    Ok(())
}

fn await_app_server_response(
    child: &mut Child,
    stdout_rx: &Receiver<Result<String, String>>,
    protocol_log: &mut String,
    events: &mut Vec<AgentEvent>,
    expected_id: u64,
    session_id: Option<&str>,
    wait: AppServerWaitOptions<'_>,
) -> Result<serde_json::Value, String> {
    let deadline = Instant::now() + wait.timeout;
    loop {
        let Some(line) = next_app_server_stdout_line(
            child,
            stdout_rx,
            deadline,
            "Codex app-server timed out waiting for protocol output",
            wait.cancel,
        )?
        else {
            return Err(format!(
                "Codex app-server exited before response id {expected_id}"
            ));
        };
        append_protocol_record(protocol_log, "stdout", line.trim_end());
        let parsed = serde_json::from_str::<serde_json::Value>(line.trim());
        if let Ok(payload) = &parsed {
            if json_rpc_id_matches(payload.get("id"), expected_id) {
                if let Some(error) = payload.get("error") {
                    return Err(format!(
                        "Codex app-server response id {expected_id} returned error: {error}"
                    ));
                }
                return Ok(payload.clone());
            }
        }

        let event = normalize_json_rpc_line(line.trim_end(), session_id);
        let kind = event.event;
        let message = event.message.clone();
        if let Some(agent_event) = event.agent_event {
            events.push(agent_event);
        }
        if is_terminal_or_fail_closed_app_server_event(kind) {
            return Err(message);
        }
    }
}

fn await_app_server_terminal_event(
    child: &mut Child,
    stdout_rx: &Receiver<Result<String, String>>,
    protocol_log: &mut String,
    events: &mut Vec<AgentEvent>,
    session_id: &str,
    stall_timeout: Duration,
    wait: AppServerWaitOptions<'_>,
) -> Result<(), String> {
    let deadline = Instant::now() + wait.timeout;
    let mut idle_deadline = Instant::now() + stall_timeout.min(wait.timeout);
    loop {
        let next_deadline = deadline.min(idle_deadline);
        let timeout_message = if next_deadline == deadline {
            "Codex app-server timed out waiting for protocol output"
        } else {
            "Codex app-server stalled waiting for turn event"
        };
        let Some(line) = next_app_server_stdout_line(
            child,
            stdout_rx,
            next_deadline,
            timeout_message,
            wait.cancel,
        )?
        else {
            return Err("Codex app-server exited before a terminal turn event".into());
        };
        idle_deadline = Instant::now() + stall_timeout;
        append_protocol_record(protocol_log, "stdout", line.trim_end());
        let event = normalize_json_rpc_line(line.trim_end(), Some(session_id));
        let kind = event.event;
        let message = event.message.clone();
        if let Some(agent_event) = event.agent_event {
            events.push(agent_event);
        }
        match kind {
            CodexAppServerEventKind::TurnCompleted => return Ok(()),
            kind if is_terminal_or_fail_closed_app_server_event(kind) => return Err(message),
            _ => {}
        }
    }
}

fn next_app_server_stdout_line(
    child: &mut Child,
    stdout_rx: &Receiver<Result<String, String>>,
    deadline: Instant,
    timeout_message: &str,
    cancel: Option<&AtomicBool>,
) -> Result<Option<String>, String> {
    loop {
        if cancel.is_some_and(|cancel| cancel.load(AtomicOrdering::Relaxed)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Codex app-server review was cancelled".into());
        }
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(timeout_message.into());
        }
        let remaining = deadline.saturating_duration_since(now);
        match stdout_rx.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(Ok(line)) => return Ok(Some(line)),
            Ok(Err(error)) => return Err(format!("Codex app-server stdout read failed: {error}")),
            Err(RecvTimeoutError::Timeout) => {
                if child
                    .try_wait()
                    .map_err(|error| error.to_string())?
                    .is_some()
                {
                    return Ok(None);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return Ok(None),
        }
    }
}

fn spawn_app_server_stdout_reader(
    stdout: std::process::ChildStdout,
) -> Receiver<Result<String, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if tx.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = tx.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    rx
}

fn is_terminal_or_fail_closed_app_server_event(kind: CodexAppServerEventKind) -> bool {
    matches!(
        kind,
        CodexAppServerEventKind::TurnFailed
            | CodexAppServerEventKind::TurnCancelled
            | CodexAppServerEventKind::TurnInputRequired
            | CodexAppServerEventKind::UnsupportedRequest
            | CodexAppServerEventKind::Malformed
    )
}

fn app_server_approval_policy(value: Option<&str>) -> serde_json::Value {
    let parsed = value.and_then(|value| serde_json::from_str(value).ok());
    match parsed {
        Some(serde_json::Value::Object(policy))
            if policy.len() == 1 && policy.contains_key("reject") =>
        {
            serde_json::Value::String("never".to_string())
        }
        Some(policy) => policy,
        None => serde_json::Value::String(value.unwrap_or("never").to_string()),
    }
}

fn app_server_turn_title(prepared: &PreparedRun) -> String {
    match (
        prepared.issue_identifier.as_deref(),
        prepared.issue_title.as_deref(),
    ) {
        (Some(identifier), Some(title)) => format!("{identifier}: {title}"),
        (Some(identifier), None) => identifier.to_string(),
        (None, Some(title)) => title.to_string(),
        (None, None) => prepared
            .workspace
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Shea Symphony Codex app-server turn")
            .to_string(),
    }
}

fn json_rpc_id_matches(value: Option<&serde_json::Value>, expected: u64) -> bool {
    match value {
        Some(serde_json::Value::Number(number)) => number.as_u64() == Some(expected),
        Some(serde_json::Value::String(text)) => text == &expected.to_string(),
        _ => false,
    }
}

fn append_protocol_record(protocol_log: &mut String, direction: &str, line: &str) {
    let escaped_line =
        serde_json::to_string(line).unwrap_or_else(|_| "\"<unserializable>\"".into());
    protocol_log.push_str(&format!(
        "{{\"direction\":\"{direction}\",\"line\":{escaped_line}}}\n"
    ));
}

#[derive(Debug, Clone)]
struct AppServerArtifactPaths {
    protocol_path: PathBuf,
    stderr_path: PathBuf,
    events_path: PathBuf,
}

#[derive(Clone, Copy)]
struct AppServerRegistryContext<'a> {
    artifacts: &'a AppServerArtifactPaths,
    prompt_artifact_path: &'a Path,
    options: AppServerRunOptions<'a>,
}

fn app_server_artifact_paths(
    prepared: &PreparedRun,
    prompt_artifact_path: &Path,
) -> AppServerArtifactPaths {
    let artifact_dir = prompt_artifact_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("/tmp"))
        .join("app-server");
    let base = format!(
        "{}-{}-attempt{}-{}",
        safe_path_component(prepared.issue_identifier.as_deref()),
        safe_path_component(prepared.run_id.as_deref()),
        prepared.attempt,
        current_time_ms()
    );
    AppServerArtifactPaths {
        protocol_path: artifact_dir.join(format!("{base}.protocol.jsonl")),
        stderr_path: artifact_dir.join(format!("{base}.stderr.log")),
        events_path: artifact_dir.join(format!("{base}.events.json")),
    }
}

fn save_app_server_session_record(
    prepared: &PreparedRun,
    artifacts: &AppServerArtifactPaths,
    prompt_artifact_path: &Path,
    thread_id: &str,
    session_id: &str,
    process_id: u32,
    status: SessionStatus,
) -> Result<(), AgentError> {
    let Some(registry_path) = prepared.session_registry_path.as_deref() else {
        return Ok(());
    };
    let now_ms = unix_timestamp_ms();
    let record = AgentSessionRecord {
        issue_id: prepared.issue_id.clone(),
        issue_identifier: prepared.issue_identifier.clone(),
        issue_title: prepared.issue_title.clone(),
        lane: prepared.lane.clone().unwrap_or_else(|| "main".into()),
        run_id: prepared.run_id.clone(),
        thread: Some(thread_id.to_string()),
        session_source: Some(crate::codex_app_server::BACKEND_NAME.into()),
        claim_value: prepared.env.get("SHEA_SYMPHONY_CLAIM").cloned(),
        actor_role: prepared.actor_role.clone(),
        actor_label: prepared.actor_label.clone(),
        git_author: prepared.git_author.clone(),
        profile_id: prepared.profile_id.clone(),
        instance_name: prepared.instance_name.clone(),
        worktree: prepared.workspace.clone(),
        branch: prepared.branch_name.clone(),
        backend: prepared.backend.clone(),
        session_name: session_id.to_string(),
        process_id: Some(process_id),
        pane_target: String::new(),
        prompt_artifact_path: prompt_artifact_path.to_path_buf(),
        log_path: artifacts.events_path.clone(),
        attach_command: format!(
            "not a tmux session; inspect app-server artifacts: protocol={} stderr={} events={}",
            artifacts.protocol_path.display(),
            artifacts.stderr_path.display(),
            artifacts.events_path.display()
        ),
        attempt: prepared.attempt.max(1),
        status,
        started_at_ms: now_ms,
        updated_at_ms: now_ms,
    };
    save_session_record(registry_path, record).map_err(|error| {
        AgentError::Unavailable(format!("app-server session registry failed: {error}"))
    })
}

fn update_app_server_session_status(
    registry_path: Option<&Path>,
    session_id: &str,
    status: SessionStatus,
) -> Result<(), AgentError> {
    let Some(registry_path) = registry_path else {
        return Ok(());
    };
    update_session_record_status(registry_path, session_id, status, unix_timestamp_ms()).map_err(
        |error| AgentError::Unavailable(format!("app-server session registry failed: {error}")),
    )
}

fn app_server_final_session_status(events: &[AgentEvent]) -> SessionStatus {
    if events
        .iter()
        .any(|event| matches!(event, AgentEvent::Completed { .. }))
    {
        return SessionStatus::Completed;
    }

    if usage_limit_pause_from_events(events).is_some() {
        return SessionStatus::UsageLimited;
    }

    SessionStatus::Failed
}

fn finish_app_server_run(
    mut child: Child,
    stderr_rx: Option<Receiver<String>>,
    events: Option<&mut Vec<AgentEvent>>,
    artifacts: &AppServerArtifactPaths,
    protocol_log: &str,
) -> Result<Option<ExitStatus>, AgentError> {
    let status = match child.try_wait()? {
        Some(status) => Some(status),
        None => {
            let _ = child.kill();
            Some(child.wait()?)
        }
    };

    fs::write(&artifacts.protocol_path, protocol_log)?;
    let stderr = stderr_rx
        .and_then(|rx| rx.recv_timeout(Duration::from_millis(500)).ok())
        .unwrap_or_default();
    fs::write(&artifacts.stderr_path, &stderr)?;
    if !stderr.trim().is_empty() {
        if let Some(events) = events {
            events.push(AgentEvent::Message {
                backend: crate::codex_app_server::BACKEND_NAME.into(),
                session_id: None,
                text: stderr,
            });
        }
    }
    Ok(status)
}

fn display_exit_status(status: Option<&ExitStatus>) -> String {
    status
        .map(|status| {
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "terminated".into())
        })
        .unwrap_or_else(|| "unknown".into())
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
        .get("SHEA_SYMPHONY_TMUX_COMMAND")
        .cloned()
        .or_else(|| std::env::var("SHEA_SYMPHONY_TMUX_COMMAND").ok())
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

    events.push(AgentEvent::SessionStarted {
        backend: prepared.backend.clone(),
        session_id: session_id.clone(),
    });
    let attach_command = format!("tmux attach-session -t {session_id}");

    let mut pipe_pane = Command::new(&tmux);
    pipe_pane
        .envs(prepared.env.iter())
        .args(["pipe-pane", "-o", "-t", target])
        .arg(format!("cat >> {}", shell_quote_path(&log_path)));
    if let Err(error) = tmux_command_status(&mut pipe_pane, "pipe-pane") {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error,
        });
        return Ok(events);
    }

    if let Some(registry_path) = prepared.session_registry_path.as_deref() {
        let now_ms = unix_timestamp_ms();
        let record = AgentSessionRecord {
            issue_id: prepared.issue_id.clone(),
            issue_identifier: prepared.issue_identifier.clone(),
            issue_title: prepared.issue_title.clone(),
            lane: prepared.lane.clone().unwrap_or_else(|| "main".into()),
            run_id: prepared.run_id.clone(),
            thread: None,
            session_source: None,
            claim_value: None,
            actor_role: prepared.actor_role.clone(),
            actor_label: prepared.actor_label.clone(),
            git_author: prepared.git_author.clone(),
            profile_id: prepared.profile_id.clone(),
            instance_name: prepared.instance_name.clone(),
            worktree: prepared.workspace.clone(),
            branch: prepared.branch_name.clone(),
            backend: prepared.backend.clone(),
            session_name: session_id.clone(),
            process_id: None,
            pane_target: target.to_string(),
            prompt_artifact_path: prompt_artifact_path.clone(),
            log_path: log_path.clone(),
            attach_command: attach_command.clone(),
            attempt: prepared.attempt.max(1),
            status: SessionStatus::Running,
            started_at_ms: now_ms,
            updated_at_ms: now_ms,
        };
        if let Err(error) = save_session_record(registry_path, record) {
            events.push(AgentEvent::Failed {
                backend: prepared.backend,
                error: format!("tmux session registry failed: {error}"),
            });
            return Ok(events);
        }
    }

    events.push(AgentEvent::Message {
        backend: prepared.backend.clone(),
        session_id: Some(session_id.clone()),
        text: format!(
            "tmux_session_evidence session={} attach_command=\"{}\" log_path={} prompt_artifact={}",
            session_id,
            attach_command,
            log_path.display(),
            prompt_artifact_path.display()
        ),
    });

    if is_codex_tmux_agent_command(agent_command) {
        match wait_for_codex_tmux_readiness(&prepared, &tmux, target) {
            Ok(()) => {}
            Err(error) => {
                events.push(AgentEvent::Failed {
                    backend: prepared.backend,
                    error,
                });
                return Ok(events);
            }
        }
    } else if is_gemini_tmux_agent_command(agent_command) {
        match wait_for_gemini_tmux_readiness(&prepared, &tmux, target) {
            Ok(()) => {}
            Err(error) => {
                events.push(AgentEvent::Failed {
                    backend: prepared.backend,
                    error,
                });
                return Ok(events);
            }
        }
    }

    let mut load_buffer = Command::new(&tmux);
    load_buffer
        .envs(prepared.env.iter())
        .args(["load-buffer", "-b", target])
        .arg(&prompt_artifact_path);
    if let Err(error) = tmux_command_status(&mut load_buffer, "load-buffer") {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error,
        });
        return Ok(events);
    }

    let mut paste_buffer = Command::new(&tmux);
    paste_buffer
        .envs(prepared.env.iter())
        .args(["paste-buffer", "-b", target, "-t", target]);
    if let Err(error) = tmux_command_status(&mut paste_buffer, "paste-buffer") {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error,
        });
        return Ok(events);
    }

    thread::sleep(Duration::from_millis(tmux_prompt_submit_delay_ms(
        &prepared,
    )));

    let mut send_enter = Command::new(&tmux);
    send_enter
        .envs(prepared.env.iter())
        .args(["send-keys", "-t", target, "C-m"]);
    if let Err(error) = tmux_command_status(&mut send_enter, "send-keys") {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error,
        });
        return Ok(events);
    }

    if is_codex_tmux_agent_command(agent_command)
        && codex_prompt_still_waiting_after_submit(&prepared, &tmux, target)
    {
        let mut send_enter = Command::new(&tmux);
        send_enter
            .envs(prepared.env.iter())
            .args(["send-keys", "-t", target, "C-m"]);
        if let Err(error) = tmux_command_status(&mut send_enter, "send-keys retry") {
            events.push(AgentEvent::Failed {
                backend: prepared.backend,
                error,
            });
            return Ok(events);
        }
    }

    events.push(AgentEvent::Message {
        backend: prepared.backend,
        session_id: Some(session_id.clone()),
        text: format!(
            "tmux_session_started session={} attach_command=\"{}\" log_path={} prompt_artifact={}",
            session_id,
            attach_command,
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

fn tmux_command_output(command: &mut Command, action: &str) -> Result<String, String> {
    match command.output() {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        }
        Ok(output) => Err(format!(
            "tmux {action} exited with status {}",
            output.status.code().unwrap_or(-1)
        )),
        Err(error) => Err(format!("tmux {action} failed: {error}")),
    }
}

fn tmux_prompt_submit_delay_ms(prepared: &PreparedRun) -> u64 {
    prepared
        .env
        .get("SHEA_SYMPHONY_TMUX_PROMPT_SUBMIT_DELAY_MS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(200)
}

fn codex_prompt_still_waiting_after_submit(
    prepared: &PreparedRun,
    tmux: &str,
    target: &str,
) -> bool {
    thread::sleep(Duration::from_millis(250));
    let Ok(capture) = capture_tmux_pane(prepared, tmux, target, DEFAULT_TMUX_CAPTURE_LINES) else {
        return false;
    };
    codex_pasted_content_waiting_for_submit(&capture)
}

fn codex_pasted_content_waiting_for_submit(text: &str) -> bool {
    let normalized = normalized_pane_text(text);
    normalized.contains("› [pasted content")
        && !normalized.contains("working")
        && !normalized.contains("thinking")
        && !normalized.contains("explored")
        && !normalized.contains("ran ")
}

fn wait_for_codex_tmux_readiness(
    prepared: &PreparedRun,
    tmux: &str,
    target: &str,
) -> Result<(), String> {
    let mut saw_trust_prompt = false;
    let mut last_capture = String::new();
    let attempts = prepared
        .env
        .get("SHEA_SYMPHONY_CODEX_TMUX_READINESS_ATTEMPTS")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(80);
    let interval_ms = prepared
        .env
        .get("SHEA_SYMPHONY_CODEX_TMUX_READINESS_INTERVAL_MS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(250);

    for _ in 0..attempts {
        let capture = capture_tmux_pane(prepared, tmux, target, DEFAULT_TMUX_CAPTURE_LINES)?;
        if codex_viewport_ready(&capture) {
            return Ok(());
        }
        if codex_workspace_trust_prompt_visible(&capture) {
            saw_trust_prompt = true;
            break;
        }
        last_capture = capture;
        thread::sleep(Duration::from_millis(interval_ms));
    }

    if !saw_trust_prompt {
        return Err(format!(
            "Codex tmux pane did not reach a ready viewport before prompt injection; last_capture={}",
            compact_pane_capture(&last_capture)
        ));
    }

    if !tmux_auto_trust_enabled(prepared) {
        return Err("Codex workspace trust prompt is visible and SHEA_SYMPHONY_TMUX_AUTO_TRUST=0 disabled auto-trust; prompt injection stopped".into());
    }

    if !workspace_is_shea_created_issue_worktree(prepared) {
        return Err(format!(
            "Codex workspace trust prompt is visible for a workspace outside the configured Shea Symphony worktree root: {}",
            prepared.workspace.display()
        ));
    }

    for key in ["C-m", "C-m"] {
        tmux_command_status(
            Command::new(tmux)
                .envs(prepared.env.iter())
                .args(["send-keys", "-t", target, key]),
            "auto-trust send-keys",
        )?;
        thread::sleep(Duration::from_millis(150));
    }

    for _ in 0..attempts {
        let capture = capture_tmux_pane(prepared, tmux, target, DEFAULT_TMUX_CAPTURE_LINES)?;
        if codex_viewport_ready(&capture) {
            return Ok(());
        }
        last_capture = capture;
        thread::sleep(Duration::from_millis(interval_ms));
    }

    Err(format!(
        "Codex workspace trust prompt could not be cleared; prompt injection stopped last_capture={}",
        compact_pane_capture(&last_capture)
    ))
}

fn wait_for_gemini_tmux_readiness(
    prepared: &PreparedRun,
    tmux: &str,
    target: &str,
) -> Result<(), String> {
    let mut last_capture = String::new();
    let attempts = prepared
        .env
        .get("SHEA_SYMPHONY_GEMINI_TMUX_READINESS_ATTEMPTS")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(80);
    let interval_ms = prepared
        .env
        .get("SHEA_SYMPHONY_GEMINI_TMUX_READINESS_INTERVAL_MS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(250);

    for _ in 0..attempts {
        let capture = capture_tmux_pane(prepared, tmux, target, DEFAULT_TMUX_CAPTURE_LINES)?;
        if gemini_viewport_ready(&capture) {
            return Ok(());
        }
        last_capture = capture;
        thread::sleep(Duration::from_millis(interval_ms));
    }

    Err(format!(
        "Gemini tmux pane did not reach a ready input viewport before prompt injection; last_capture={}",
        compact_pane_capture(&last_capture)
    ))
}

fn capture_tmux_pane(
    prepared: &PreparedRun,
    tmux: &str,
    target: &str,
    max_lines: usize,
) -> Result<String, String> {
    let start = format!("-{}", max_lines.clamp(1, 500));
    tmux_command_output(
        Command::new(tmux).envs(prepared.env.iter()).args([
            "capture-pane",
            "-p",
            "-t",
            target,
            "-S",
            &start,
        ]),
        "capture-pane",
    )
}

fn is_codex_tmux_agent_command(agent_command: &str) -> bool {
    agent_command
        .split_whitespace()
        .next()
        .is_some_and(|word| word.ends_with("codex") || word == "codex")
}

fn is_gemini_tmux_agent_command(agent_command: &str) -> bool {
    agent_command
        .split_whitespace()
        .next()
        .is_some_and(|word| word.ends_with("gemini") || word == "gemini")
}

fn tmux_auto_trust_enabled(prepared: &PreparedRun) -> bool {
    let value = prepared
        .env
        .get("SHEA_SYMPHONY_TMUX_AUTO_TRUST")
        .cloned()
        .or_else(|| std::env::var("SHEA_SYMPHONY_TMUX_AUTO_TRUST").ok());
    !matches!(value.as_deref(), Some("0"))
}

fn workspace_is_shea_created_issue_worktree(prepared: &PreparedRun) -> bool {
    let Some(root) = prepared
        .env
        .get("SHEA_SYMPHONY_WORKSPACE_ROOT")
        .filter(|value| !value.trim().is_empty())
    else {
        return false;
    };
    let root = canonical_path_or_self(Path::new(root));
    let workspace = canonical_path_or_self(&prepared.workspace);
    workspace.starts_with(root)
}

fn canonical_path_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn codex_workspace_trust_prompt_visible(text: &str) -> bool {
    let normalized = normalized_pane_text(text);
    normalized.contains("do you trust the contents of this directory")
        || normalized.contains("do you trust the files in this directory")
        || normalized.contains("do you trust the files in this folder")
        || (normalized.contains("trust")
            && normalized.contains("directory")
            && normalized.contains("codex"))
}

fn codex_viewport_ready(text: &str) -> bool {
    let normalized = normalized_pane_text(text);
    !normalized.is_empty()
        && !codex_workspace_trust_prompt_visible(text)
        && (normalized.contains("codex")
            || normalized.contains("type a message")
            || normalized.contains("send a message")
            || normalized.contains("what can i help")
            || normalized.contains("approval")
            || normalized.contains("model")
            || text.contains('›')
            || text.contains('▌'))
}

fn gemini_viewport_ready(text: &str) -> bool {
    let normalized = normalized_pane_text(text);
    !normalized.is_empty()
        && !normalized.contains("waiting for authentication")
        && !normalized.contains("press esc or ctrl+c to cancel")
        && (normalized.contains("type your message")
            || normalized.contains("@path/to/file")
            || normalized.contains("auto-accept edits")
            || normalized.contains("/model")
            || normalized.contains("quota"))
}

fn normalized_pane_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn compact_pane_capture(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_LEN: usize = 180;
    if compact.len() > MAX_LEN {
        format!("{}...", &compact[..MAX_LEN])
    } else {
        compact
    }
}

fn tmux_agent_shell_command(
    prepared: &PreparedRun,
    agent_command: &str,
    prompt_artifact_path: &Path,
) -> String {
    format!(
        "SHEA_SYMPHONY_PROMPT_PATH={} SHEA_SYMPHONY_ACTOR_ROLE={} SHEA_SYMPHONY_ACTOR_LABEL={} SHEA_SYMPHONY_GIT_AUTHOR={} sh -lc {}",
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
        .get("SHEA_SYMPHONY_TMUX_SESSION_PREFIX")
        .map(String::as_str)
        .unwrap_or("shea");
    deterministic_session_name(
        prefix,
        prepared
            .lane
            .as_deref()
            .or_else(|| {
                prepared
                    .env
                    .get("SHEA_SYMPHONY_AGENT_LANE")
                    .map(String::as_str)
            })
            .unwrap_or("main"),
        prepared.issue_identifier.as_deref(),
        prepared.attempt,
        prepared.issue_title.as_deref().or_else(|| {
            prepared
                .workspace
                .file_name()
                .and_then(|name| name.to_str())
        }),
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

pub(crate) fn message_field(events: &[AgentEvent], prefix: &str) -> Option<String> {
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
        .join("shea-symphony")
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

pub(crate) fn is_codex_app_server_command(command: &str) -> bool {
    codex_app_server_executable(command).is_some()
}

pub(crate) fn codex_app_server_executable(command: &str) -> Option<String> {
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

    let executable = tokens.get(index)?;
    let first_arg = tokens.get(index + 1)?;

    (is_codex_executable(executable) && *first_arg == "app-server").then(|| executable.to_string())
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
        log_path: message_field(events, "normalized_events_artifact=")
            .or_else(|| message_field(events, "log_path="))
            .map(PathBuf::from),
        attach_command: None,
    }
}

pub fn usage_limit_pause_from_events(events: &[AgentEvent]) -> Option<UsageLimitPause> {
    events.iter().find_map(|event| match event {
        AgentEvent::Failed { error, .. } => classify_usage_limit_text(error),
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ARTIFACT_COUNTER: AtomicU64 = AtomicU64::new(0);

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
            "---\nagent:\n  backend: tmux\ntmux:\n  command: /usr/local/bin/tmux\n  agent_command: codex\n  session_prefix: shea-test\n---\nPrompt",
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
                .get("SHEA_SYMPHONY_TMUX_COMMAND")
                .map(String::as_str),
            Some("/usr/local/bin/tmux")
        );
        assert_eq!(
            prepared
                .env
                .get("SHEA_SYMPHONY_TMUX_SESSION_PREFIX")
                .map(String::as_str),
            Some("shea-test")
        );
    }

    #[test]
    fn tmux_summary_reports_pending_attachable_session() {
        let events = vec![
            AgentEvent::SessionStarted {
                backend: "tmux".into(),
                session_id: "shea-main-220".into(),
            },
            AgentEvent::Message {
                backend: "tmux".into(),
                session_id: Some("shea-main-220".into()),
                text: "tmux_session_started session=shea-main-220 log_path=/tmp/shea.log".into(),
            },
        ];

        let summary = TmuxBackend.summarize(&events);

        assert!(!summary.success);
        assert!(summary.pending_session);
        assert_eq!(
            summary.attach_command.as_deref(),
            Some("tmux attach-session -t shea-main-220")
        );
        assert_eq!(summary.log_path, Some(PathBuf::from("/tmp/shea.log")));
    }

    #[test]
    fn detects_codex_workspace_trust_prompt_conservatively() {
        assert!(codex_workspace_trust_prompt_visible(
            "Codex\nDo you trust the contents of this directory?\n"
        ));
        assert!(codex_workspace_trust_prompt_visible(
            "Codex asks: do you trust the files in this folder?"
        ));
        assert!(codex_viewport_ready("Codex\n› ready"));
        assert!(!codex_viewport_ready(
            "Codex\nDo you trust the contents of this directory?"
        ));
    }

    #[test]
    fn detects_codex_pasted_prompt_still_waiting_for_submit() {
        assert!(codex_pasted_content_waiting_for_submit(
            "╭ OpenAI Codex ╮\n\n› [Pasted Content 24237 chars]\n"
        ));
        assert!(!codex_pasted_content_waiting_for_submit(
            "› [Pasted Content 24237 chars]\n\n• Working (18s • esc to interrupt)"
        ));
    }

    #[test]
    fn detects_gemini_ready_viewport_conservatively() {
        assert!(!gemini_viewport_ready(
            "Gemini\nWaiting for authentication...\nPress Esc or Ctrl+C to cancel"
        ));
        assert!(!gemini_viewport_ready("Gemini CLI starting up"));
        assert!(gemini_viewport_ready(
            "Gemini\nType your message or @path/to/file"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn tmux_backend_auto_advances_codex_trust_prompt_before_prompt_injection() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let fake_tmux = temp.path().join("fake-tmux.sh");
        let log_path = temp.path().join("fake-tmux.log");
        let state_path = temp.path().join("fake-tmux-state");
        fs::write(&fake_tmux, fake_tmux_script(false)).unwrap();
        let mut perms = fs::metadata(&fake_tmux).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_tmux, perms).unwrap();
        let workspace_root = temp.path().join("worktrees");
        let workspace = workspace_root.join("issue-230-auto-trust");
        fs::create_dir_all(&workspace).unwrap();
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {:?}\nagent:\n  backend: tmux\ntmux:\n  command: {:?}\n  agent_command: codex\n  session_prefix: shea-test\n---\nPrompt",
                workspace_root.display().to_string(),
                fake_tmux.display().to_string()
            ),
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md"))
                .unwrap();
        let backend = TmuxBackend;
        let mut prepared = backend
            .prepare(workspace, "echo tmux-auto-trust".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/smoke.prompt.md"));
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared
            .env
            .insert("FAKE_TMUX_LOG".into(), log_path.display().to_string());
        prepared
            .env
            .insert("FAKE_TMUX_STATE".into(), state_path.display().to_string());
        prepared.env.insert(
            "SHEA_SYMPHONY_CODEX_TMUX_READINESS_ATTEMPTS".into(),
            "2".into(),
        );
        prepared.env.insert(
            "SHEA_SYMPHONY_CODEX_TMUX_READINESS_INTERVAL_MS".into(),
            "1".into(),
        );

        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);
        let fake_log = fs::read_to_string(log_path).unwrap();

        assert!(summary.pending_session, "{fake_log}");
        assert_before(&fake_log, "capture-pane", "load-buffer");
        assert_before(&fake_log, "send-keys -t", "load-buffer");
        assert!(fake_log.contains("load-buffer"), "{fake_log}");
        assert!(fake_log.contains("paste-buffer"), "{fake_log}");
        assert_eq!(fake_log.matches(" C-m").count(), 3, "{fake_log}");
    }

    #[cfg(unix)]
    #[test]
    fn tmux_backend_waits_for_gemini_readiness_before_prompt_injection() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let fake_tmux = temp.path().join("fake-tmux.sh");
        let log_path = temp.path().join("fake-tmux.log");
        let state_path = temp.path().join("fake-tmux-state");
        fs::write(&fake_tmux, fake_tmux_script(false)).unwrap();
        let mut perms = fs::metadata(&fake_tmux).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_tmux, perms).unwrap();
        let workspace_root = temp.path().join("worktrees");
        let workspace = workspace_root.join("issue-282-gemini-ready");
        fs::create_dir_all(&workspace).unwrap();
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {:?}\nagent:\n  backend: tmux\ntmux:\n  command: {:?}\n  agent_command: gemini\n  session_prefix: shea-test\n---\nPrompt",
                workspace_root.display().to_string(),
                fake_tmux.display().to_string()
            ),
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md"))
                .unwrap();
        let backend = TmuxBackend;
        let mut prepared = backend
            .prepare(workspace, "echo tmux-gemini-ready".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/smoke.prompt.md"));
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared
            .env
            .insert("FAKE_TMUX_LOG".into(), log_path.display().to_string());
        prepared
            .env
            .insert("FAKE_TMUX_STATE".into(), state_path.display().to_string());
        prepared
            .env
            .insert("FAKE_TMUX_MODE".into(), "gemini".into());
        prepared.env.insert(
            "SHEA_SYMPHONY_GEMINI_TMUX_READINESS_INTERVAL_MS".into(),
            "1".into(),
        );

        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);
        let fake_log = fs::read_to_string(log_path).unwrap();

        assert!(summary.pending_session, "{fake_log}");
        assert!(fake_log.matches("capture-pane").count() >= 2, "{fake_log}");
        assert_before(&fake_log, "capture-pane", "load-buffer");
        assert!(fake_log.contains("load-buffer"), "{fake_log}");
        assert!(fake_log.contains("paste-buffer"), "{fake_log}");
        assert!(fake_log.contains(" C-m"), "{fake_log}");
    }

    #[cfg(unix)]
    #[test]
    fn tmux_backend_fails_closed_when_gemini_readiness_is_not_observed() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let fake_tmux = temp.path().join("fake-tmux.sh");
        let log_path = temp.path().join("fake-tmux.log");
        let state_path = temp.path().join("fake-tmux-state");
        fs::write(&fake_tmux, fake_tmux_script(false)).unwrap();
        let mut perms = fs::metadata(&fake_tmux).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_tmux, perms).unwrap();
        let workspace_root = temp.path().join("worktrees");
        let workspace = workspace_root.join("issue-282-gemini-stuck");
        fs::create_dir_all(&workspace).unwrap();
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {:?}\nagent:\n  backend: tmux\ntmux:\n  command: {:?}\n  agent_command: gemini\n  session_prefix: shea-test\n---\nPrompt",
                workspace_root.display().to_string(),
                fake_tmux.display().to_string()
            ),
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md"))
                .unwrap();
        let backend = TmuxBackend;
        let mut prepared = backend
            .prepare(workspace, "echo tmux-gemini-stuck".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/smoke.prompt.md"));
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared
            .env
            .insert("FAKE_TMUX_LOG".into(), log_path.display().to_string());
        prepared
            .env
            .insert("FAKE_TMUX_STATE".into(), state_path.display().to_string());
        prepared
            .env
            .insert("FAKE_TMUX_MODE".into(), "gemini-auth".into());
        prepared.env.insert(
            "SHEA_SYMPHONY_GEMINI_TMUX_READINESS_ATTEMPTS".into(),
            "2".into(),
        );
        prepared.env.insert(
            "SHEA_SYMPHONY_GEMINI_TMUX_READINESS_INTERVAL_MS".into(),
            "1".into(),
        );

        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);
        let fake_log = fs::read_to_string(log_path).unwrap();

        assert!(!summary.success);
        assert!(!summary.pending_session);
        assert!(summary.message.contains("Gemini tmux pane did not reach"));
        assert!(summary.message.contains("Waiting for authentication"));
        assert!(!fake_log.contains("load-buffer"), "{fake_log}");
        assert!(!fake_log.contains("paste-buffer"), "{fake_log}");
    }

    #[cfg(unix)]
    #[test]
    fn tmux_backend_fails_closed_when_auto_trust_is_disabled() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let fake_tmux = temp.path().join("fake-tmux.sh");
        let log_path = temp.path().join("fake-tmux.log");
        let state_path = temp.path().join("fake-tmux-state");
        fs::write(&fake_tmux, fake_tmux_script(false)).unwrap();
        let mut perms = fs::metadata(&fake_tmux).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_tmux, perms).unwrap();
        let workspace_root = temp.path().join("worktrees");
        let workspace = workspace_root.join("issue-230-auto-trust-disabled");
        fs::create_dir_all(&workspace).unwrap();
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {:?}\nagent:\n  backend: tmux\ntmux:\n  command: {:?}\n  agent_command: codex\n  session_prefix: shea-test\n---\nPrompt",
                workspace_root.display().to_string(),
                fake_tmux.display().to_string()
            ),
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md"))
                .unwrap();
        let backend = TmuxBackend;
        let mut prepared = backend
            .prepare(workspace, "echo tmux-auto-trust".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/smoke.prompt.md"));
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared
            .env
            .insert("FAKE_TMUX_LOG".into(), log_path.display().to_string());
        prepared
            .env
            .insert("FAKE_TMUX_STATE".into(), state_path.display().to_string());
        prepared
            .env
            .insert("SHEA_SYMPHONY_TMUX_AUTO_TRUST".into(), "0".into());

        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);
        let fake_log = fs::read_to_string(log_path).unwrap();

        assert!(!summary.success);
        assert!(!summary.pending_session);
        assert!(summary.session_id.is_some());
        assert!(summary.attach_command.is_some());
        assert!(summary.log_path.is_some());
        assert!(summary.message.contains("SHEA_SYMPHONY_TMUX_AUTO_TRUST=0"));
        assert!(!fake_log.contains("load-buffer"), "{fake_log}");
        assert!(!fake_log.contains("paste-buffer"), "{fake_log}");
    }

    #[cfg(unix)]
    #[test]
    fn tmux_backend_fails_closed_when_trust_prompt_cannot_be_cleared() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let fake_tmux = temp.path().join("fake-tmux.sh");
        let log_path = temp.path().join("fake-tmux.log");
        let state_path = temp.path().join("fake-tmux-state");
        fs::write(&fake_tmux, fake_tmux_script(true)).unwrap();
        let mut perms = fs::metadata(&fake_tmux).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_tmux, perms).unwrap();
        let workspace_root = temp.path().join("worktrees");
        let workspace = workspace_root.join("issue-230-auto-trust-stuck");
        fs::create_dir_all(&workspace).unwrap();
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {:?}\nagent:\n  backend: tmux\ntmux:\n  command: {:?}\n  agent_command: codex\n  session_prefix: shea-test\n---\nPrompt",
                workspace_root.display().to_string(),
                fake_tmux.display().to_string()
            ),
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md"))
                .unwrap();
        let backend = TmuxBackend;
        let mut prepared = backend
            .prepare(workspace, "echo tmux-auto-trust".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/smoke.prompt.md"));
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared
            .env
            .insert("FAKE_TMUX_LOG".into(), log_path.display().to_string());
        prepared
            .env
            .insert("FAKE_TMUX_STATE".into(), state_path.display().to_string());
        prepared.env.insert(
            "SHEA_SYMPHONY_CODEX_TMUX_READINESS_ATTEMPTS".into(),
            "2".into(),
        );
        prepared.env.insert(
            "SHEA_SYMPHONY_CODEX_TMUX_READINESS_INTERVAL_MS".into(),
            "1".into(),
        );

        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);
        let fake_log = fs::read_to_string(log_path).unwrap();

        assert!(!summary.success);
        assert!(!summary.pending_session);
        assert!(summary.session_id.is_some());
        assert!(summary.attach_command.is_some());
        assert!(summary.message.contains("could not be cleared"));
        assert_eq!(fake_log.matches(" C-m").count(), 2, "{fake_log}");
        assert!(!fake_log.contains("load-buffer"), "{fake_log}");
        assert!(!fake_log.contains("paste-buffer"), "{fake_log}");
    }

    #[test]
    fn tmux_session_name_uses_lane_metadata_when_present() {
        let mut prepared = PreparedRun {
            backend: "tmux".into(),
            workspace: PathBuf::from("/tmp/review-workspace"),
            prompt: "prompt".into(),
            prompt_artifact_path: None,
            command: Some("codex".into()),
            timeout_ms: 0,
            stall_timeout_ms: 0,
            model: None,
            reasoning_effort: None,
            approval_policy: None,
            sandbox: None,
            turn_sandbox_policy: None,
            app_server_resume_thread_id: None,
            profile_id: None,
            instance_name: None,
            env: BTreeMap::from([(
                "SHEA_SYMPHONY_TMUX_SESSION_PREFIX".into(),
                "shea-test".into(),
            )]),
            actor_role: None,
            actor_label: None,
            git_author: None,
            issue_id: None,
            issue_identifier: Some("#220".into()),
            issue_title: Some("Add tmux-backed local agent runtime for all lanes".into()),
            lane: Some("review".into()),
            run_id: None,
            attempt: 2,
            branch_name: None,
            session_registry_path: None,
        };

        let session = tmux_session_name(&prepared);
        assert!(session.starts_with("shea-test-review-220-attempt-2-"));
        prepared.lane = None;
        prepared
            .env
            .insert("SHEA_SYMPHONY_AGENT_LANE".into(), "merge".into());
        let session = tmux_session_name(&prepared);
        assert!(session.starts_with("shea-test-merge-220-attempt-2-"));
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
        let artifact_root = temp.path().join("artifacts");
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nagent:\n  backend: tmux\nartifacts:\n  root: {:?}\n  namespace: test/repo\ntmux:\n  command: tmux\n  agent_command: cat > shea-prompt.txt\n  session_prefix: shea-test\n---\nPrompt",
                artifact_root.display().to_string()
            ),
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
        prepared.issue_id = Some("I_225".into());
        prepared.issue_identifier = Some("#225".into());
        prepared.issue_title = Some("Add durable tmux session registry".into());
        prepared.lane = Some("main".into());
        prepared.attempt = 2;
        prepared.branch_name = Some("feature/issue-225".into());
        let tmux_tmp = temp.path().join("tmux-tmp");
        fs::create_dir_all(&tmux_tmp).unwrap();
        let probe_session = format!("shea-test-probe-{}", current_time_ms());
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
            .contains("tmux attach-session -t shea-test-main-225-attempt-2"));
        assert!(summary
            .log_path
            .as_ref()
            .unwrap()
            .display()
            .to_string()
            .ends_with(".log"));
        let registry = crate::session_registry::load_session_registry(
            &crate::session_registry::session_registry_path(&config),
        )
        .unwrap();
        assert_eq!(registry.sessions.len(), 1);
        assert_eq!(
            registry.sessions[0].issue_identifier.as_deref(),
            Some("#225")
        );
        assert_eq!(registry.sessions[0].lane, "main");
        assert_eq!(
            registry.sessions[0].branch.as_deref(),
            Some("feature/issue-225")
        );
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

    #[cfg(unix)]
    fn fake_tmux_script(always_trust: bool) -> String {
        let trust_condition = if always_trust {
            "true"
        } else {
            "[ \"$count\" -lt 2 ]"
        };
        format!(
            r#"#!/bin/sh
set -eu
log="${{FAKE_TMUX_LOG:?}}"
state="${{FAKE_TMUX_STATE:?}}"
mode="${{FAKE_TMUX_MODE:-codex}}"
printf '%s\n' "$*" >> "$log"
cmd="${{1:-}}"
case "$cmd" in
  new-session|pipe-pane|load-buffer|paste-buffer)
    exit 0
    ;;
  send-keys)
    last=""
    for arg in "$@"; do last="$arg"; done
    if [ "$last" = "C-m" ]; then
      count="$(cat "$state" 2>/dev/null || echo 0)"
      count=$((count + 1))
      printf '%s\n' "$count" > "$state"
    fi
    exit 0
    ;;
  capture-pane)
    if [ "$mode" = "gemini" ]; then
      count="$(cat "$state" 2>/dev/null || echo 0)"
      count=$((count + 1))
      printf '%s\n' "$count" > "$state"
      if [ "$count" -lt 2 ]; then
        printf '%s\n' 'Gemini' 'Waiting for authentication...' 'Press Esc or Ctrl+C to cancel'
      else
        printf '%s\n' 'Gemini' 'Type your message or @path/to/file'
      fi
      exit 0
    fi
    if [ "$mode" = "gemini-auth" ]; then
      printf '%s\n' 'Gemini' 'Waiting for authentication...' 'Press Esc or Ctrl+C to cancel'
      exit 0
    fi
    count="$(cat "$state" 2>/dev/null || echo 0)"
    if {trust_condition}; then
      printf '%s\n' 'Codex' 'Do you trust the contents of this directory?'
    else
      printf '%s\n' 'Codex' '› ready'
    fi
    exit 0
    ;;
esac
exit 0
"#
        )
    }

    fn assert_before(haystack: &str, left: &str, right: &str) {
        let left_index = haystack
            .find(left)
            .unwrap_or_else(|| panic!("missing {left:?} in {haystack}"));
        let right_index = haystack
            .find(right)
            .unwrap_or_else(|| panic!("missing {right:?} in {haystack}"));
        assert!(
            left_index < right_index,
            "expected {left:?} before {right:?} in {haystack}"
        );
    }

    #[test]
    fn classifies_usage_limit_text_conservatively() {
        let pause = classify_usage_limit_text("Error: usage limit reached for this model").unwrap();
        assert_eq!(pause.classifier, "usage_limit");
        assert!(pause.evidence.contains("usage limit"));

        let pause = classify_usage_limit_text("HTTP 429 quota exceeded").unwrap();
        assert_eq!(pause.classifier, "quota_exceeded");

        assert!(classify_usage_limit_text("HTTP 429: too many requests").is_none());

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
                text: "test review::tests::classifies_quota_rate_limit_as_wait_and_retry ... ok"
                    .into(),
            },
            AgentEvent::Failed {
                backend: "codex".into(),
                error: "turn/failed: Resource exhausted, please retry later.".into(),
            },
        ];

        let pause = usage_limit_pause_from_events(&events).unwrap();
        assert_eq!(pause.classifier, "resource_exhausted");
    }

    #[test]
    fn usage_limit_pause_ignores_non_failure_messages() {
        let events = vec![AgentEvent::Message {
            backend: "codex-app-server".into(),
            session_id: Some("s1".into()),
            text: "test review::tests::classifies_quota_rate_limit_as_wait_and_retry ... ok".into(),
        }];

        assert!(usage_limit_pause_from_events(&events).is_none());
    }

    fn codex_config(command: &str, timeout_ms: u64) -> RuntimeConfig {
        let artifact_root = test_artifact_root();
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\nartifacts:\n  root: {:?}\nagent:\n  backend: codex\ncodex:\n  command: {command:?}\n  turn_timeout_ms: {timeout_ms}\n---\nPrompt",
                artifact_root.display().to_string()
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn codex_config_with_stall(
        command: &str,
        timeout_ms: u64,
        stall_timeout_ms: u64,
    ) -> RuntimeConfig {
        let artifact_root = test_artifact_root();
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\nartifacts:\n  root: {:?}\nagent:\n  backend: codex\ncodex:\n  command: {command:?}\n  turn_timeout_ms: {timeout_ms}\n  stall_timeout_ms: {stall_timeout_ms}\n---\nPrompt",
                artifact_root.display().to_string()
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn test_artifact_root() -> PathBuf {
        let counter = TEST_ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "shea-symphony-agent-test-{}-{counter}",
            std::process::id()
        ))
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

    #[cfg(unix)]
    fn fake_claude_stream_json(root: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = root.join("fake-claude");
        fs::write(
            &script,
            r#"#!/bin/sh
trace="${FAKE_CLAUDE_TRACE:?}"
mode="${FAKE_CLAUDE_MODE:-fixture}"
mkdir -p "$trace"
pwd > "$trace/cwd"
printf 'ARG=%s\n' "$@" > "$trace/args"
if [ "$mode" = timeout ]; then
  sleep 5 &
  child=$!
  printf '%s' "$child" > "$trace/child-pid"
  wait "$child"
  exit 0
fi
cat > "$trace/input.jsonl"
case "$mode" in
  fixture)
    cat "${FAKE_CLAUDE_FIXTURE:?}"
    ;;
  startup_failure)
    printf '%s\n' 'fake Claude startup failed' >&2
    exit 23
    ;;
esac
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
        script
    }

    fn claude_fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/backends/claude-main")
            .join(name)
    }

    fn codex_config_with_profile(command: &str) -> RuntimeConfig {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/profiles/cockpit-tools-codex-instances.json");
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

    #[cfg(unix)]
    fn fake_codex_app_server(root: &Path) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let codex = bin_dir.join("codex");
        let trace_path = root.join("fake-codex.trace");
        fs::write(
            &codex,
            r#"#!/bin/sh
trace_file="${FAKE_CODEX_TRACE:-/tmp/fake-codex.trace}"
mode="${FAKE_CODEX_MODE:-completed}"
count=0
printf '%s\n' 'fake codex stderr' >&2

while IFS= read -r line; do
  count=$((count + 1))
  printf 'JSON:%s\n' "$line" >> "$trace_file"

  case "$count" in
    1)
      printf '%s\n' '{"id":1,"result":{}}'
      ;;
    2)
      # initialized notification; no response required
      ;;
    3)
      if printf '%s\n' "$line" | grep -q '"method":"thread/resume"'; then
        printf '%s\n' '{"id":2,"result":{"thread":{"id":"thread-368"}}}'
      else
        printf '%s\n' '{"id":2,"result":{"thread":{"id":"thread-368"}}}'
      fi
      ;;
    4)
      if printf '%s\n' "$line" | grep -q '"text":"Continue"'; then
        printf '%s\n' '{"id":3,"result":{"turn":{"id":"turn-resume"}}}'
      else
        printf '%s\n' '{"id":3,"result":{"turn":{"id":"turn-368"}}}'
      fi
      case "$mode" in
        completed)
          printf '%s\n' '{"method":"thread/tokenUsage/updated","params":{"inputTokens":4,"outputTokens":5,"totalTokens":9}}'
          printf '%s\n' '{"method":"turn/completed","params":{"turn":{"status":"completed"}}}'
          exit 0
          ;;
        input_required)
          printf '%s\n' '{"method":"turn/input_required","params":{"reason":"blocked"}}'
          exit 0
          ;;
        tool_call)
          printf '%s\n' '{"method":"item/tool/call","id":7,"params":{"name":"linear_graphql","arguments":{}}}'
          exit 0
          ;;
        failed)
          printf '%s\n' '{"method":"turn/failed","params":{"error":{"message":"boom"}}}'
          exit 0
          ;;
        cancelled)
          printf '%s\n' '{"method":"turn/cancelled","params":{"message":"operator stopped"}}'
          exit 0
          ;;
        silent_after_turn_start)
          sleep 5
          exit 0
          ;;
        partial_malformed)
          printf '%s' '{"method":"thread/tokenUsage/updated","params":{"inputTokens":1,'
          printf '%s\n' '"outputTokens":2,"totalTokens":3}}'
          printf '%s\n' 'not-json'
          exit 0
          ;;
      esac
      ;;
  esac
done
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&codex).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&codex, permissions).unwrap();
        (codex, trace_path)
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
        assert!(!temp.path().join("SHEA_SYMPHONY_PROMPT.md").exists());
    }

    #[test]
    fn subprocess_backend_uses_external_prompt_artifact_path() {
        let temp = tempfile::tempdir().unwrap();
        let prompt_path = temp.path().join("logs").join("prompts").join("prompt.md");
        let config = codex_config(
            "printf '%s' \"$SHEA_SYMPHONY_PROMPT_PATH\" > prompt_path.txt",
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
            .join("SHEA_SYMPHONY_PROMPT.md")
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn codex_backend_runs_app_server_protocol_and_records_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let (codex, trace_path) = fake_codex_app_server(temp.path());
        let workspace = temp.path().join("workspace");
        let workspace_cwd = workspace.display().to_string();
        fs::create_dir_all(&workspace).unwrap();
        let config = codex_config(&format!("{} app-server", codex.display()), 5_000);
        let backend = CodexBackend;
        let mut prepared = backend
            .prepare(workspace, "hello app-server".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/app-server.prompt.md"));
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared.issue_identifier = Some("#368".into());
        prepared.issue_title = Some("Add Codex app-server transport backend harness".into());
        prepared
            .env
            .insert("FAKE_CODEX_TRACE".into(), trace_path.display().to_string());
        prepared
            .env
            .insert("FAKE_CODEX_MODE".into(), "completed".into());
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(summary.success);
        assert_eq!(summary.session_id.as_deref(), Some("thread-368-turn-368"));
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::TokenUsage {
                input_tokens: 4,
                output_tokens: 5,
                total_tokens: 9,
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::Completed { summary, .. }
                if summary.contains("completed with status completed")
        )));

        let protocol_path = message_field(&events, "protocol_artifact=").unwrap();
        let stderr_path = message_field(&events, "stderr_artifact=").unwrap();
        let events_path = message_field(&events, "normalized_events_artifact=").unwrap();
        assert_eq!(summary.log_path.as_deref(), Some(Path::new(&events_path)));
        assert!(fs::read_to_string(protocol_path)
            .unwrap()
            .contains("turn/completed"));
        assert!(fs::read_to_string(stderr_path)
            .unwrap()
            .contains("fake codex stderr"));
        assert!(fs::read_to_string(events_path)
            .unwrap()
            .contains("SessionStarted"));

        let trace = fs::read_to_string(trace_path).unwrap();
        assert!(trace.contains("\"method\":\"initialize\""));
        assert!(trace.contains("\"method\":\"thread/start\""));
        assert!(trace.contains("\"method\":\"turn/start\""));
        assert!(trace.contains("\"approvalPolicy\":\"never\""));
        assert!(trace.contains(&format!(
            "\"cwd\":{}",
            serde_json::to_string(&workspace_cwd).unwrap()
        )));
        assert_eq!(trace.matches("\"reasoningEffort\":\"high\"").count(), 2);
        assert!(trace.contains("hello app-server"));
    }

    #[cfg(unix)]
    #[test]
    fn codex_backend_resumes_existing_app_server_thread_with_continue_turn() {
        let temp = tempfile::tempdir().unwrap();
        let (codex, trace_path) = fake_codex_app_server(temp.path());
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let config = codex_config(&format!("{} app-server", codex.display()), 5_000);
        let backend = CodexBackend;
        let mut prepared = backend
            .prepare(workspace, "Continue".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/app-server.prompt.md"));
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared.issue_identifier = Some("#368".into());
        prepared.app_server_resume_thread_id = Some("thread-368".into());
        prepared
            .env
            .insert("FAKE_CODEX_TRACE".into(), trace_path.display().to_string());
        prepared
            .env
            .insert("FAKE_CODEX_MODE".into(), "completed".into());

        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);
        let trace = fs::read_to_string(trace_path).unwrap();

        assert!(summary.success);
        assert_eq!(
            summary.session_id.as_deref(),
            Some("thread-368-turn-resume")
        );
        assert!(!trace.contains("\"method\":\"thread/start\""));
        assert!(trace.contains("\"method\":\"thread/resume\""));
        assert!(trace.contains("\"method\":\"turn/start\""));
        assert!(trace.contains("\"threadId\":\"thread-368\""));
        assert!(trace.contains("\"text\":\"Continue\""));
    }

    #[test]
    fn app_server_approval_policy_uses_current_schema_and_legacy_reject_fallback() {
        assert_eq!(
            app_server_approval_policy(Some("\"on-request\"")),
            serde_json::json!("on-request")
        );
        assert_eq!(
            app_server_approval_policy(Some("never")),
            serde_json::json!("never")
        );
        assert_eq!(
            app_server_approval_policy(Some(
                r#"{"reject":{"sandbox_approval":true,"rules":true,"mcp_elicitations":true}}"#
            )),
            serde_json::json!("never")
        );
        assert_eq!(app_server_approval_policy(None), serde_json::json!("never"));
    }

    #[cfg(unix)]
    #[test]
    fn codex_app_server_records_session_registry_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let (codex, trace_path) = fake_codex_app_server(temp.path());
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let config = codex_config(&format!("{} app-server", codex.display()), 5_000);
        let backend = CodexBackend;
        let registry_path = temp.path().join("sessions/session-registry.json");
        let mut prepared = backend
            .prepare(workspace.clone(), "hello app-server".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/app-server.prompt.md"));
        prepared.issue_id = Some("issue-368".into());
        prepared.issue_identifier = Some("#368".into());
        prepared.issue_title = Some("Add Codex app-server transport backend harness".into());
        prepared.lane = Some("main".into());
        prepared.run_id = Some("run-368".into());
        prepared.session_registry_path = Some(registry_path.clone());
        prepared.branch_name = Some("feature/issue-368".into());
        prepared.env.insert(
            "SHEA_SYMPHONY_CLAIM".into(),
            "v=1 lane=main issue=#368".into(),
        );
        prepared
            .env
            .insert("FAKE_CODEX_TRACE".into(), trace_path.display().to_string());
        prepared
            .env
            .insert("FAKE_CODEX_MODE".into(), "completed".into());

        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);
        let registry = crate::session_registry::load_session_registry(&registry_path).unwrap();
        let record = registry
            .sessions
            .iter()
            .find(|record| record.session_name == "thread-368-turn-368")
            .expect("app-server session record");

        assert!(summary.success);
        assert_eq!(record.backend, "codex");
        assert_eq!(record.lane, "main");
        assert_eq!(record.thread.as_deref(), Some("thread-368"));
        assert_eq!(record.session_source.as_deref(), Some("codex-app-server"));
        assert!(record.process_id.is_some());
        assert_eq!(record.status, SessionStatus::Completed);
        assert_eq!(record.worktree, workspace);
        assert!(record.attach_command.contains("not a tmux session"));
        assert!(record
            .log_path
            .display()
            .to_string()
            .contains("/logs/app-server/368-"));
        assert!(record
            .prompt_artifact_path
            .ends_with("app-server.prompt.md"));
    }

    #[cfg(unix)]
    #[test]
    fn codex_backend_app_server_fails_closed_for_input_required() {
        for mode in ["input_required", "tool_call"] {
            let temp = tempfile::tempdir().unwrap();
            let (codex, trace_path) = fake_codex_app_server(temp.path());
            let workspace = temp.path().join("workspace");
            fs::create_dir_all(&workspace).unwrap();
            let config = codex_config(&format!("{} app-server", codex.display()), 5_000);
            let backend = CodexBackend;
            let mut prepared = backend
                .prepare(workspace, "needs input".into(), &config)
                .unwrap();
            prepared.prompt_artifact_path =
                Some(temp.path().join("logs/prompts/app-server.prompt.md"));
            prepared.session_registry_path =
                Some(temp.path().join("sessions/session-registry.json"));
            prepared
                .env
                .insert("FAKE_CODEX_TRACE".into(), trace_path.display().to_string());
            prepared.env.insert("FAKE_CODEX_MODE".into(), mode.into());
            let events = backend.run(prepared).unwrap();
            let summary = backend.summarize(&events);

            assert!(!summary.success, "{mode}");
            assert!(
                summary.message.contains("requires unavailable user input"),
                "{mode}: {summary:?}"
            );
            assert!(message_field(&events, "protocol_artifact=").is_some());
        }
    }

    #[cfg(unix)]
    #[test]
    fn codex_backend_app_server_handles_failed_and_cancelled_turns() {
        for (mode, expected) in [("failed", "boom"), ("cancelled", "operator stopped")] {
            let temp = tempfile::tempdir().unwrap();
            let (codex, trace_path) = fake_codex_app_server(temp.path());
            let workspace = temp.path().join("workspace");
            fs::create_dir_all(&workspace).unwrap();
            let config = codex_config(&format!("{} app-server", codex.display()), 5_000);
            let backend = CodexBackend;
            let mut prepared = backend
                .prepare(workspace, format!("{mode} turn"), &config)
                .unwrap();
            prepared.prompt_artifact_path =
                Some(temp.path().join("logs/prompts/app-server.prompt.md"));
            prepared.session_registry_path =
                Some(temp.path().join("sessions/session-registry.json"));
            prepared
                .env
                .insert("FAKE_CODEX_TRACE".into(), trace_path.display().to_string());
            prepared.env.insert("FAKE_CODEX_MODE".into(), mode.into());
            let events = backend.run(prepared).unwrap();
            let summary = backend.summarize(&events);

            assert!(!summary.success, "{mode}");
            assert!(summary.message.contains(expected), "{mode}: {summary:?}");
            assert!(message_field(&events, "exit_status=").is_some());
        }
    }

    #[cfg(unix)]
    #[test]
    fn codex_backend_app_server_fails_fast_when_turn_stalls_after_start() {
        let temp = tempfile::tempdir().unwrap();
        let (codex, trace_path) = fake_codex_app_server(temp.path());
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let config = codex_config_with_stall(&format!("{} app-server", codex.display()), 5_000, 50);
        let backend = CodexBackend;
        let mut prepared = backend
            .prepare(workspace, "silent turn".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/app-server.prompt.md"));
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared
            .env
            .insert("FAKE_CODEX_TRACE".into(), trace_path.display().to_string());
        prepared
            .env
            .insert("FAKE_CODEX_MODE".into(), "silent_after_turn_start".into());

        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(!summary.success);
        assert!(summary.message.contains("stalled waiting for turn event"));
        let protocol_path = message_field(&events, "protocol_artifact=").unwrap();
        let protocol = fs::read_to_string(protocol_path).unwrap();
        assert!(protocol.contains("turn/start"));
        assert!(!protocol.contains("turn/completed"));
    }

    #[cfg(unix)]
    #[test]
    fn codex_backend_app_server_buffers_partial_lines_and_fails_on_malformed_protocol() {
        let temp = tempfile::tempdir().unwrap();
        let (codex, trace_path) = fake_codex_app_server(temp.path());
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let config = codex_config(&format!("{} app-server", codex.display()), 5_000);
        let backend = CodexBackend;
        let mut prepared = backend
            .prepare(workspace, "partial stream".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/app-server.prompt.md"));
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared
            .env
            .insert("FAKE_CODEX_TRACE".into(), trace_path.display().to_string());
        prepared
            .env
            .insert("FAKE_CODEX_MODE".into(), "partial_malformed".into());
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(!summary.success);
        assert!(summary.message.contains("malformed"));
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::TokenUsage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: 3,
                ..
            }
        )));
    }

    #[test]
    fn codex_app_server_command_guard_is_specific() {
        assert!(is_codex_app_server_command("codex app-server"));
        assert!(is_codex_app_server_command(
            "codex app-server -c 'service_tier=\"fast\"'"
        ));
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
            prepared.env.get("SHEA_SYMPHONY_PROFILE_HOME"),
            Some(&"/tmp/cockpit/codex-alpha".into())
        );
        assert!(!prepared.env.contains_key("CODEX_HOME"));
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

    #[cfg(unix)]
    #[test]
    fn claude_code_backend_runs_stream_json_and_records_session_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let claude = fake_claude_stream_json(temp.path());
        let config = claude_config(&claude.display().to_string(), 5_000);
        let backend = ClaudeCodeBackend;
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let trace = temp.path().join("trace");
        let registry_path = temp.path().join("sessions/session-registry.json");
        let mut prepared = backend
            .prepare(workspace.clone(), "hello claude".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/claude.prompt.md"));
        prepared.session_registry_path = Some(registry_path.clone());
        prepared.issue_id = Some("issue-510".into());
        prepared.issue_identifier = Some("#510".into());
        prepared.issue_title = Some("Run Claude Code stream-json sessions".into());
        prepared.lane = Some("main".into());
        prepared.run_id = Some("run-510".into());
        prepared.branch_name = Some("feature/issue-510".into());
        prepared
            .env
            .insert("FAKE_CLAUDE_TRACE".into(), trace.display().to_string());
        prepared.env.insert(
            "FAKE_CLAUDE_FIXTURE".into(),
            claude_fixture("success.jsonl").display().to_string(),
        );
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(summary.success);
        assert_eq!(summary.backend, "claude-code");
        assert_eq!(summary.session_id.as_deref(), Some("claude-session-510"));
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::Message { text, .. } if text.contains("claude_tool_use name=Read")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::Message { text, .. } if text.contains("claude_tool_result")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::TokenUsage {
                input_tokens: 12,
                output_tokens: 7,
                total_tokens: 19,
                ..
            }
        )));
        let args = fs::read_to_string(trace.join("args")).unwrap();
        for argument in [
            "ARG=-p",
            "ARG=--input-format",
            "ARG=stream-json",
            "ARG=--output-format",
            "ARG=--verbose",
        ] {
            assert!(args.contains(argument), "{args}");
        }
        assert_eq!(
            fs::canonicalize(fs::read_to_string(trace.join("cwd")).unwrap().trim()).unwrap(),
            fs::canonicalize(&workspace).unwrap()
        );
        let input: serde_json::Value = serde_json::from_str(
            fs::read_to_string(trace.join("input.jsonl"))
                .unwrap()
                .trim(),
        )
        .unwrap();
        assert_eq!(
            input.pointer("/type").and_then(serde_json::Value::as_str),
            Some("user")
        );
        assert_eq!(
            input
                .pointer("/message/content/0/text")
                .and_then(serde_json::Value::as_str),
            Some("hello claude")
        );

        let registry = crate::session_registry::load_session_registry(&registry_path).unwrap();
        let record = registry.sessions.first().unwrap();
        assert_eq!(record.session_name, "claude-session-510");
        assert_eq!(record.thread.as_deref(), Some("claude-session-510"));
        assert_eq!(
            record.session_source.as_deref(),
            Some(CLAUDE_STREAM_JSON_SOURCE)
        );
        assert_eq!(record.issue_identifier.as_deref(), Some("#510"));
        assert_eq!(record.run_id.as_deref(), Some("run-510"));
        assert_eq!(record.worktree, workspace);
        assert_eq!(record.status, SessionStatus::Completed);
        let protocol_path = message_field(&events, "protocol_artifact=").unwrap();
        assert!(fs::read_to_string(protocol_path)
            .unwrap()
            .contains("claude-session-510"));
    }

    #[cfg(unix)]
    #[test]
    fn claude_code_backend_resumes_recorded_session_through_cli_contract() {
        let temp = tempfile::tempdir().unwrap();
        let claude = fake_claude_stream_json(temp.path());
        let config = claude_config(&claude.display().to_string(), 5_000);
        let backend = ClaudeCodeBackend;
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let trace = temp.path().join("trace");
        let mut prepared = backend
            .prepare(workspace, "Continue".into(), &config)
            .unwrap();
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared.env.insert(
            CLAUDE_RESUME_SESSION_ENV.into(),
            "claude-session-510".into(),
        );
        prepared
            .env
            .insert("FAKE_CLAUDE_TRACE".into(), trace.display().to_string());
        prepared.env.insert(
            "FAKE_CLAUDE_FIXTURE".into(),
            claude_fixture("success.jsonl").display().to_string(),
        );
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(summary.success);
        let args = fs::read_to_string(trace.join("args")).unwrap();
        assert!(args.contains("ARG=--resume"), "{args}");
        assert!(args.contains("ARG=claude-session-510"), "{args}");
    }

    #[cfg(unix)]
    #[test]
    fn claude_code_backend_fails_closed_for_protocol_and_process_failures() {
        for (mode, fixture, expected) in [
            ("fixture", "error.jsonl", "permission was denied"),
            ("fixture", "cancelled.jsonl", "cancelled"),
            ("fixture", "malformed.jsonl", "malformed"),
            ("fixture", "truncated.jsonl", "before a terminal result"),
            (
                "startup_failure",
                "success.jsonl",
                "startup failed with status 23",
            ),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let claude = fake_claude_stream_json(temp.path());
            let config = claude_config(&claude.display().to_string(), 5_000);
            let workspace = temp.path().join("workspace");
            fs::create_dir_all(&workspace).unwrap();
            let trace = temp.path().join("trace");
            let backend = ClaudeCodeBackend;
            let mut prepared = backend
                .prepare(workspace, "prompt".into(), &config)
                .unwrap();
            prepared.session_registry_path =
                Some(temp.path().join("sessions/session-registry.json"));
            prepared
                .env
                .insert("FAKE_CLAUDE_TRACE".into(), trace.display().to_string());
            prepared.env.insert("FAKE_CLAUDE_MODE".into(), mode.into());
            prepared.env.insert(
                "FAKE_CLAUDE_FIXTURE".into(),
                claude_fixture(fixture).display().to_string(),
            );

            let events = backend.run(prepared).unwrap();
            let summary = backend.summarize(&events);

            assert!(!summary.success, "{mode} {fixture}: {summary:?}");
            assert!(
                summary.message.contains(expected),
                "{mode} {fixture}: {summary:?}"
            );
            assert!(message_field(&events, "protocol_artifact=").is_some());
            assert!(message_field(&events, "normalized_events_artifact=").is_some());
        }
    }

    #[cfg(unix)]
    #[test]
    fn claude_code_backend_times_out_and_terminates_process_group() {
        let temp = tempfile::tempdir().unwrap();
        let claude = fake_claude_stream_json(temp.path());
        let config = claude_config(&claude.display().to_string(), 1_000);
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let trace = temp.path().join("trace");
        let backend = ClaudeCodeBackend;
        let mut prepared = backend
            .prepare(workspace, "prompt".into(), &config)
            .unwrap();
        prepared.session_registry_path = Some(temp.path().join("sessions/session-registry.json"));
        prepared
            .env
            .insert("FAKE_CLAUDE_TRACE".into(), trace.display().to_string());
        prepared
            .env
            .insert("FAKE_CLAUDE_MODE".into(), "timeout".into());

        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(!summary.success);
        assert!(summary.message.contains("timed out"));
        let child_pid = fs::read_to_string(trace.join("child-pid"))
            .unwrap()
            .parse::<i32>()
            .unwrap();
        let mut process_exists = true;
        for _ in 0..20 {
            // Signal 0 performs a read-only existence check for the former descendant.
            process_exists = unsafe { libc::kill(child_pid, 0) == 0 };
            if !process_exists {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process_exists,
            "timed-out descendant {child_pid} is still alive"
        );
    }

    #[ignore = "requires an operator-configured local Claude Code command"]
    #[test]
    fn claude_code_live_local_worktree_uat() {
        let command = std::env::var("SHEA_CLAUDE_UAT_COMMAND")
            .expect("set SHEA_CLAUDE_UAT_COMMAND to a local Claude executable or wrapper");
        let temp = tempfile::tempdir().unwrap();
        let config = claude_config(&command, 600_000);
        let backend = ClaudeCodeBackend;

        for (lane, initial, expected, prompt) in [
            (
                "main",
                "before\n",
                "main stream-json uat\n",
                "Replace uat.txt with exactly `main stream-json uat` followed by a newline, then verify it with a shell command.",
            ),
            (
                "merge",
                "<<<<<<< HEAD\napproved\n=======\nstale\n>>>>>>> origin/main\n",
                "approved\n",
                "Resolve the conflict markers in uat.txt by preserving `approved`, then verify that no conflict markers remain.",
            ),
        ] {
            let workspace = temp.path().join(lane);
            fs::create_dir_all(&workspace).unwrap();
            fs::write(workspace.join("uat.txt"), initial).unwrap();
            let mut prepared = backend
                .prepare(workspace.clone(), prompt.into(), &config)
                .unwrap();
            prepared.prompt_artifact_path =
                Some(temp.path().join(format!("logs/prompts/{lane}.prompt.md")));
            prepared.session_registry_path =
                Some(temp.path().join("sessions/session-registry.json"));
            prepared.issue_id = Some(format!("local-{lane}-uat"));
            prepared.issue_identifier = Some(format!("local-{lane}-uat"));
            prepared.lane = Some(lane.into());
            prepared.run_id = Some(format!("local-{lane}-uat"));

            let events = backend.run(prepared).unwrap();
            let summary = backend.summarize(&events);

            assert!(summary.success, "{lane}: {summary:?}");
            assert_eq!(fs::read_to_string(workspace.join("uat.txt")).unwrap(), expected);
            assert!(summary.session_id.is_some(), "{lane}: {summary:?}");
            assert!(summary.log_path.as_ref().is_some_and(|path| path.is_file()));
        }
    }
}
