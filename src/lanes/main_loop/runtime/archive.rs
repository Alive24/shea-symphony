use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::model::TrackerIssue;
use jade_symphony::runtime_state::{runtime_state_path, RuntimeState};

use crate::current_time_ms;

use super::ResumePreflightAction;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeWorkspaceStatus {
    Absent,
    Clean(PathBuf),
    Dirty(PathBuf),
    Unknown { path: PathBuf, reason: String },
}

pub(super) fn stale_runtime_state_action(
    state: &RuntimeState,
    issue: &TrackerIssue,
    normalized_state: &str,
    config: &RuntimeConfig,
) -> Result<ResumePreflightAction, Box<dyn std::error::Error>> {
    let active_issue = state
        .active_issue
        .as_ref()
        .ok_or("runtime state has no active issue")?;
    let archive_reason = if config
        .terminal_state_set()
        .iter()
        .any(|state| state == normalized_state)
    {
        "tracker_state_terminal"
    } else if matches!(normalized_state, "agent review" | "human review") {
        "tracker_state_handoff"
    } else {
        "tracker_state_non_active"
    };

    match runtime_workspace_status(state)? {
        RuntimeWorkspaceStatus::Absent | RuntimeWorkspaceStatus::Clean(_) => {
            Ok(ResumePreflightAction::ArchiveStale {
                issue_identifier: active_issue.identifier.clone(),
                tracker_state: issue.state.clone(),
                archive_reason: archive_reason.into(),
            })
        }
        RuntimeWorkspaceStatus::Dirty(path) => Ok(ResumePreflightAction::Block {
            reason: format!(
                "runtime state references {} but tracker state is {}; workspace is dirty at {}",
                active_issue.identifier,
                issue.state,
                path.display()
            ),
        }),
        RuntimeWorkspaceStatus::Unknown { path, reason } => Ok(ResumePreflightAction::Block {
            reason: format!(
                "runtime state references {} but tracker state is {}; workspace status is unknown at {}: {}",
                active_issue.identifier,
                issue.state,
                path.display(),
                reason
            ),
        }),
    }
}

fn runtime_workspace_status(
    state: &RuntimeState,
) -> Result<RuntimeWorkspaceStatus, Box<dyn std::error::Error>> {
    let Some(path) = state.workspace_path.as_ref() else {
        return Ok(RuntimeWorkspaceStatus::Absent);
    };
    if !path.exists() {
        return Ok(RuntimeWorkspaceStatus::Absent);
    }
    if !path.is_dir() {
        return Ok(RuntimeWorkspaceStatus::Unknown {
            path: path.clone(),
            reason: "workspace path is not a directory".into(),
        });
    }

    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(path)
        .arg("status")
        .arg("--porcelain")
        .output();
    match output {
        Ok(output) if output.status.success() => {
            if output.stdout.is_empty() {
                Ok(RuntimeWorkspaceStatus::Clean(path.clone()))
            } else {
                Ok(RuntimeWorkspaceStatus::Dirty(path.clone()))
            }
        }
        Ok(output) => Ok(RuntimeWorkspaceStatus::Unknown {
            path: path.clone(),
            reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
        Err(error) => Ok(RuntimeWorkspaceStatus::Unknown {
            path: path.clone(),
            reason: error.to_string(),
        }),
    }
}

pub(super) fn archive_runtime_state(
    config: &RuntimeConfig,
    state: &RuntimeState,
    reason: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let runtime_path = runtime_state_path(config);
    let archive_dir = runtime_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("archive");
    std::fs::create_dir_all(&archive_dir)?;
    let issue_ref = state
        .active_issue
        .as_ref()
        .map(|issue| issue.identifier.as_str())
        .unwrap_or("unknown");
    let archive_path = archive_dir.join(format!(
        "runtime-state-{}-{}-{}.json",
        current_time_ms(),
        sanitize_archive_segment(issue_ref),
        sanitize_archive_segment(reason)
    ));
    std::fs::write(&archive_path, serde_json::to_string_pretty(state)?)?;
    Ok(archive_path)
}

fn sanitize_archive_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    sanitized.trim_matches('-').to_string()
}
