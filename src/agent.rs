use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use std::{fs, io::Write};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;
use crate::model::AgentEvent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedRun {
    pub backend: String,
    pub workspace: PathBuf,
    pub prompt: String,
    pub command: Option<String>,
    pub timeout_ms: u64,
    pub approval_policy: Option<String>,
    pub sandbox: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSummary {
    pub backend: String,
    pub success: bool,
    pub session_id: Option<String>,
    pub message: String,
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
        _config: &RuntimeConfig,
    ) -> Result<PreparedRun, AgentError> {
        Ok(PreparedRun {
            backend: self.name().into(),
            workspace,
            prompt: rendered_prompt,
            command: None,
            timeout_ms: 0,
            approval_policy: None,
            sandbox: None,
        })
    }

    fn run(&self, prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
        Ok(vec![
            AgentEvent::SessionStarted {
                backend: prepared.backend.clone(),
                session_id: "dry-run-session".into(),
            },
            AgentEvent::Completed {
                backend: prepared.backend,
                session_id: Some("dry-run-session".into()),
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
            session_id: Some("dry-run-session".into()),
            message: "dry-run complete".into(),
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
        Ok(PreparedRun {
            backend: self.name().into(),
            workspace,
            prompt: rendered_prompt,
            command: Some(config.codex.command.clone()),
            timeout_ms: config.codex.turn_timeout_ms,
            approval_policy: Some(config.codex.approval_policy.to_string()),
            sandbox: Some(config.codex.thread_sandbox.clone()),
        })
    }

    fn run(&self, prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
        run_subprocess_backend(prepared, "codex-subprocess")
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
        Ok(PreparedRun {
            backend: format!("claude-code:{}", config.claude.command),
            workspace,
            prompt: rendered_prompt,
            command: None,
            timeout_ms: 0,
            approval_policy: None,
            sandbox: None,
        })
    }

    fn run(&self, _prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
        Err(AgentError::Unavailable(
            "Claude Code backend is preserved as a peer backend but delayed".into(),
        ))
    }

    fn stop(&self, _reason: &str) -> Result<(), AgentError> {
        Ok(())
    }

    fn summarize(&self, _events: &[AgentEvent]) -> AgentSummary {
        AgentSummary {
            backend: self.name().into(),
            success: false,
            session_id: None,
            message: "Claude Code backend not executed".into(),
        }
    }
}

fn run_subprocess_backend(
    prepared: PreparedRun,
    session_id: &str,
) -> Result<Vec<AgentEvent>, AgentError> {
    let mut events = vec![AgentEvent::SessionStarted {
        backend: prepared.backend.clone(),
        session_id: session_id.into(),
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

    fs::write(
        prepared.workspace.join("JADE_SYMPHONY_PROMPT.md"),
        &prepared.prompt,
    )?;
    let mut child = Command::new("sh")
        .arg("-lc")
        .arg(command)
        .current_dir(&prepared.workspace)
        .env("JADE_SYMPHONY_PROMPT_PATH", "JADE_SYMPHONY_PROMPT.md")
        .env(
            "JADE_SYMPHONY_APPROVAL_POLICY",
            prepared.approval_policy.as_deref().unwrap_or_default(),
        )
        .env(
            "JADE_SYMPHONY_SANDBOX",
            prepared.sandbox.as_deref().unwrap_or_default(),
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
                    session_id: Some(session_id.into()),
                    text: stdout,
                });
            }
            if !stderr.trim().is_empty() {
                events.push(AgentEvent::Message {
                    backend: prepared.backend.clone(),
                    session_id: Some(session_id.into()),
                    text: stderr,
                });
            }
            if output.status.success() {
                events.push(AgentEvent::Completed {
                    backend: prepared.backend,
                    session_id: Some(session_id.into()),
                    summary: "Codex subprocess completed successfully.".into(),
                });
            } else {
                events.push(AgentEvent::Failed {
                    backend: prepared.backend,
                    error: format!(
                        "Codex subprocess exited with status {}",
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
                error: format!("Codex subprocess timed out after {}ms", prepared.timeout_ms),
            });
            return Ok(events);
        }

        thread::sleep(Duration::from_millis(10));
    }
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
        session_id,
        message: failure
            .or(completed)
            .unwrap_or_else(|| "no terminal event".into()),
    }
}

pub fn backend_from_config(config: &RuntimeConfig) -> Box<dyn AgentBackend> {
    match config.backend.kind.as_str() {
        "codex" => Box::<CodexBackend>::default(),
        "claude-code" => Box::<ClaudeCodeBackend>::default(),
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
}
