use std::io;
use std::path::Path;

use jade_symphony::config::RuntimeConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MainAppServerSmokeGate {
    pub(crate) backend: String,
    pub(crate) backend_source: String,
    pub(crate) command: String,
    pub(crate) approval_policy: String,
    pub(crate) app_server_live_smoke_ready: bool,
    pub(crate) app_server_live_smoke_reason: String,
}

pub(crate) fn main_app_server_smoke_gate(config: &RuntimeConfig) -> MainAppServerSmokeGate {
    match config.backend.kind.as_str() {
        "codex" if config.codex.command.contains("app-server") => MainAppServerSmokeGate {
            backend: config.backend.kind.clone(),
            backend_source: "codex-app-server".into(),
            command: config.codex.command.clone(),
            approval_policy: codex_approval_policy_label(config),
            app_server_live_smoke_ready: true,
            app_server_live_smoke_reason:
                "main_lane.backend=codex and codex.command includes app-server".into(),
        },
        "codex" => MainAppServerSmokeGate {
            backend: config.backend.kind.clone(),
            backend_source: "codex-subprocess".into(),
            command: config.codex.command.clone(),
            approval_policy: codex_approval_policy_label(config),
            app_server_live_smoke_ready: false,
            app_server_live_smoke_reason: "codex command does not select the app-server transport"
                .into(),
        },
        "tmux" => MainAppServerSmokeGate {
            backend: config.backend.kind.clone(),
            backend_source: "tmux-fallback".into(),
            command: config
                .tmux
                .main_agent_command
                .clone()
                .unwrap_or_else(|| config.tmux.agent_command.clone()),
            approval_policy: "n/a".into(),
            app_server_live_smoke_ready: false,
            app_server_live_smoke_reason:
                "tmux is explicit fallback/debug and is not the app-server smoke path".into(),
        },
        "dry-run" => MainAppServerSmokeGate {
            backend: config.backend.kind.clone(),
            backend_source: "dry-run".into(),
            command: "dry-run".into(),
            approval_policy: "n/a".into(),
            app_server_live_smoke_ready: false,
            app_server_live_smoke_reason: "dry-run backend cannot perform a live app-server smoke"
                .into(),
        },
        other => MainAppServerSmokeGate {
            backend: config.backend.kind.clone(),
            backend_source: other.into(),
            command: other.into(),
            approval_policy: "n/a".into(),
            app_server_live_smoke_ready: false,
            app_server_live_smoke_reason:
                "configured Main backend is not the Codex app-server runtime".into(),
        },
    }
}

pub(crate) fn ensure_write_mode_main_agent_backend(
    workflow_path: &Path,
    config: &RuntimeConfig,
    command: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if config.backend.kind != "dry-run" {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "write-mode {command} is blocked because workflow={} configures main_lane.backend=dry-run; configure a real main-agent backend such as tmux, codex, or claude-code before using --write",
            workflow_path.display()
        ),
    )
    .into())
}

fn codex_approval_policy_label(config: &RuntimeConfig) -> String {
    config
        .codex
        .approval_policy
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| config.codex.approval_policy.to_string())
}
