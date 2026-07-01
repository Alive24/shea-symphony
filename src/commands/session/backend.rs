use shea_symphony::agent::{
    AgentBackend, ClaudeCodeBackend, CodexBackend, DryRunBackend, TmuxBackend,
};
use shea_symphony::config::RuntimeConfig;

use super::AgentSessionLaneArg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSessionBackendSpec {
    pub(crate) backend: String,
    pub(crate) command: String,
}

pub(super) fn validate_tmux_session_config(
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if config.tmux.command.trim().is_empty() {
        return Err("tmux.command must not be empty for session start".into());
    }
    if config.tmux.agent_command.trim().is_empty() {
        return Err("tmux.agent_command must not be empty for session start".into());
    }
    if config.tmux.session_prefix.trim().is_empty() {
        return Err("tmux.session_prefix must not be empty for session start".into());
    }
    Ok(())
}

pub(crate) fn agent_session_backend_spec(
    config: &RuntimeConfig,
    lane: AgentSessionLaneArg,
) -> Result<AgentSessionBackendSpec, Box<dyn std::error::Error>> {
    match lane {
        AgentSessionLaneArg::Main => main_agent_session_backend_spec(config, lane),
        AgentSessionLaneArg::Review => tmux_agent_session_backend_spec(config, lane),
        AgentSessionLaneArg::Merge => merge_agent_session_backend_spec(config, lane),
    }
}

fn main_agent_session_backend_spec(
    config: &RuntimeConfig,
    lane: AgentSessionLaneArg,
) -> Result<AgentSessionBackendSpec, Box<dyn std::error::Error>> {
    let backend = match config.backend.kind.as_str() {
        "codex" => "codex",
        "tmux" => "tmux",
        "claude-code" => "claude-code",
        "dry-run" => "dry-run",
        other => {
            return Err(format!(
                "unsupported main_lane.backend `{other}`; expected codex, tmux, claude-code, or dry-run"
            )
            .into())
        }
    };
    let command = match backend {
        "codex" => non_empty_session_command(
            &config.codex.command,
            "codex.command must not be empty for main session start",
        )?,
        "tmux" => {
            validate_tmux_session_config(config)?;
            tmux_agent_command_for_lane(config, lane)?
        }
        "claude-code" => non_empty_session_command(
            &config.claude.command,
            "claude.command must not be empty for main session start",
        )?,
        "dry-run" => "dry-run".into(),
        _ => unreachable!("validated main agent backend"),
    };

    Ok(AgentSessionBackendSpec {
        backend: backend.into(),
        command,
    })
}

fn tmux_agent_session_backend_spec(
    config: &RuntimeConfig,
    lane: AgentSessionLaneArg,
) -> Result<AgentSessionBackendSpec, Box<dyn std::error::Error>> {
    validate_tmux_session_config(config)?;
    Ok(AgentSessionBackendSpec {
        backend: "tmux".into(),
        command: tmux_agent_command_for_lane(config, lane)?,
    })
}

fn merge_agent_session_backend_spec(
    config: &RuntimeConfig,
    lane: AgentSessionLaneArg,
) -> Result<AgentSessionBackendSpec, Box<dyn std::error::Error>> {
    let requested = config.merge_lane.agent_backend.trim();
    let backend = match requested {
        "" | "codex" | "codex-app-server" | "app-server" => "codex",
        "tmux" => "tmux",
        "claude-code" => "claude-code",
        "dry-run" => "dry-run",
        other => {
            return Err(format!(
                "unsupported merge_lane.agent_backend `{other}`; expected codex, tmux, claude-code, or dry-run"
            )
            .into())
        }
    };
    let command = match backend {
        "codex" => non_empty_session_command(
            &config.codex.command,
            "codex.command must not be empty for merge session start",
        )?,
        "tmux" => {
            validate_tmux_session_config(config)?;
            tmux_agent_command_for_lane(config, lane)?
        }
        "claude-code" => non_empty_session_command(
            &config.claude.command,
            "claude.command must not be empty for merge session start",
        )?,
        "dry-run" => "dry-run".into(),
        _ => unreachable!("validated merge agent backend"),
    };

    Ok(AgentSessionBackendSpec {
        backend: backend.into(),
        command,
    })
}

fn non_empty_session_command(
    value: &str,
    message: &'static str,
) -> Result<String, Box<dyn std::error::Error>> {
    let command = value.trim();
    if command.is_empty() {
        Err(message.into())
    } else {
        Ok(command.to_string())
    }
}

pub(crate) fn agent_session_backend(
    backend: &str,
) -> Result<Box<dyn AgentBackend>, Box<dyn std::error::Error>> {
    match backend {
        "codex" => Ok(Box::<CodexBackend>::default()),
        "claude-code" => Ok(Box::<ClaudeCodeBackend>::default()),
        "tmux" => Ok(Box::<TmuxBackend>::default()),
        "dry-run" => Ok(Box::<DryRunBackend>::default()),
        other => Err(format!("unsupported agent session backend `{other}`").into()),
    }
}

pub(crate) fn tmux_agent_command_for_lane(
    config: &RuntimeConfig,
    lane: AgentSessionLaneArg,
) -> Result<String, Box<dyn std::error::Error>> {
    let command = match lane {
        AgentSessionLaneArg::Main => config
            .tmux
            .main_agent_command
            .as_deref()
            .unwrap_or(&config.tmux.agent_command),
        AgentSessionLaneArg::Review => config
            .tmux
            .review_agent_command
            .as_deref()
            .or_else(|| {
                (config.review.backend == "gemini-cli")
                    .then_some(config.review.gemini_command.as_str())
            })
            .or_else(|| {
                (config.review.backend == "agy-cli").then_some(config.review.agy_command.as_str())
            })
            .unwrap_or(&config.tmux.agent_command),
        AgentSessionLaneArg::Merge => config
            .tmux
            .merge_agent_command
            .as_deref()
            .unwrap_or(&config.tmux.agent_command),
    };

    if command.trim().is_empty() {
        return Err(format!(
            "tmux {} agent command must not be empty for session start",
            lane.label()
        )
        .into());
    }

    Ok(command.to_string())
}
