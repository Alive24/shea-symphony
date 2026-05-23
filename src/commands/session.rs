use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use clap::ValueEnum;
use jade_symphony::config::RuntimeConfig;
use jade_symphony::lane_claim::LaneClaimLane;
use jade_symphony::model::TrackerIssue;
use jade_symphony::workflow::{AgentLane, WorkflowDefinition};
use jade_symphony::workspace::safe_identifier;

mod backend;
mod claim;
mod start;

#[cfg(test)]
pub(crate) use backend::tmux_agent_command_for_lane;
use backend::validate_tmux_session_config;
pub(crate) use backend::{agent_session_backend, agent_session_backend_spec};
pub(crate) use claim::{
    lane_claim_command, matching_lane_claim_for_session, record_manual_lane_claim_evidence,
    timeline_claim_actor, timeline_claim_run,
};
#[cfg(test)]
pub(crate) use claim::{lane_claim_for_manual_worker, validate_lane_claim_state};
pub(crate) use start::{
    agent_session_start, legacy_agent_session_start, record_agent_session_events,
    rendered_lane_prompt_artifact_path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum AgentSessionLaneArg {
    Main,
    Review,
    Merge,
}

impl AgentSessionLaneArg {
    pub(crate) fn workflow_lane(self) -> AgentLane {
        match self {
            Self::Main => AgentLane::MainAgent,
            Self::Review => AgentLane::ReviewAgent,
            Self::Merge => AgentLane::MergeAgent,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Review => "review",
            Self::Merge => "merge",
        }
    }

    pub(crate) fn claim_field(self) -> &'static str {
        match self {
            Self::Main => "Main Agent",
            Self::Review => "Review Agent",
            Self::Merge => "Merging Agent",
        }
    }

    pub(crate) fn claim_lane(self) -> LaneClaimLane {
        match self {
            Self::Main => LaneClaimLane::Main,
            Self::Review => LaneClaimLane::Review,
            Self::Merge => LaneClaimLane::Merge,
        }
    }
}

pub(crate) fn timeline_pr_summary(issue: &TrackerIssue) -> String {
    issue
        .linked_pull_requests
        .iter()
        .find_map(
            |pull_request| match (pull_request.number, pull_request.url.as_deref()) {
                (Some(number), Some(url)) => Some(format!("#{number} {url}")),
                (Some(number), None) => Some(format!("#{number}")),
                (None, Some(url)) => Some(url.to_string()),
                (None, None) => None,
            },
        )
        .unwrap_or_else(|| "not recorded".into())
}

pub(crate) fn agent_session_list(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    validate_tmux_session_config(&config)?;

    let output = ProcessCommand::new(&config.tmux.command)
        .args(["list-sessions", "-F", "#{session_name}:#{session_attached}"])
        .output();
    let Ok(output) = output else {
        println!("agent_session_list=unavailable reason=tmux_not_executable");
        return Ok(());
    };
    if !output.status.success() {
        println!("agent_session_list=none");
        return Ok(());
    }

    let prefix = format!("{}-", safe_identifier(&config.tmux.session_prefix));
    let mut found = false;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let (session, attached) = line.split_once(':').unwrap_or((line, "0"));
        if !session.starts_with(&prefix) {
            continue;
        }
        found = true;
        println!(
            "agent_session session={} attached={} attach_command=\"{} attach-session -t {}\"",
            session, attached, config.tmux.command, session
        );
    }
    if !found {
        println!("agent_session_list=none");
    }
    Ok(())
}

pub(crate) fn agent_session_attach(
    workflow_path: PathBuf,
    session: String,
    exec: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    validate_tmux_session_config(&config)?;

    let attach_command = format!("{} attach-session -t {}", config.tmux.command, session);
    println!("attach_command={attach_command}");
    if exec {
        let status = ProcessCommand::new(&config.tmux.command)
            .args(["attach-session", "-t", &session])
            .status()?;
        if !status.success() {
            return Err(format!(
                "tmux attach-session exited with status {}",
                status.code().unwrap_or(-1)
            )
            .into());
        }
    }
    Ok(())
}
