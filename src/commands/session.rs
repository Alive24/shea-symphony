use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use clap::ValueEnum;
use jade_symphony::agent::{
    AgentBackend, AgentSummary, ClaudeCodeBackend, CodexBackend, DryRunBackend, TmuxBackend,
};
use jade_symphony::config::RuntimeConfig;
use jade_symphony::event_log::{EventLog, EventRecord};
use jade_symphony::lane_claim::{
    LaneClaim, LaneClaimActor, LaneClaimLane, LaneClaimSource, LaneClaimState,
};
use jade_symphony::model::{native_subissue_gate_blocker, AgentEvent, TrackerIssue};
use jade_symphony::profiles::selected_execution_profile;
use jade_symphony::session_registry::{
    save_session_record, session_registry_path, unix_timestamp_ms, AgentSessionRecord,
    SessionStatus,
};
use jade_symphony::tracker::{adapter_from_config, ProjectFieldAssignment, TrackerAdapter};
use jade_symphony::workflow::{AgentLane, WorkflowDefinition};
use jade_symphony::workspace::{
    apply_local_git_identity, prepare_workspace, profile_scoped_identifier, safe_identifier,
    GitIdentityApplyResult,
};

use crate::cli::CliLaneClaimSource;
use crate::{
    add_timeline_comment_with_recovery, append_tracker_mutation_audit, current_git_branch,
    current_gmt_timestamp, current_time_ms, load_config,
    preflight_canonical_checkout_for_write_mode, project_text_field, recovery_key,
    render_parseable_lane_claim, render_prompt_with_claim, set_project_field_with_recovery,
    shell_quote_display, stable_recovery_hash, upsert_workpad_with_recovery, TrackerMutationAudit,
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

pub(crate) fn lane_claim_command(
    workflow_path: PathBuf,
    issue_ref: String,
    lane: AgentSessionLaneArg,
    worker: String,
    source: CliLaneClaimSource,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if worker.trim().is_empty() {
        return Err("lane claim requires a non-empty --worker".into());
    }

    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(
        &config,
        &format!("{} claim", lane.label()),
        write,
    )?;

    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    validate_lane_claim_state(&issue, lane, &config)?;

    let existing_value = project_text_field(&issue, lane.claim_field());
    let claim = lane_claim_for_manual_worker(
        &issue,
        lane,
        actor_from_worker(&worker),
        source.into(),
        worker.trim(),
        existing_value.as_deref(),
    )?;
    let claim_value = render_parseable_lane_claim(&claim)?;

    if !write {
        println!(
            "{}_claim_dry_run action=claim_field issue_ref={} field={:?} run={} value={claim_value}",
            lane.label(),
            issue.identifier,
            lane.claim_field(),
            claim.run
        );
        return Ok(());
    }

    let outcome = set_project_field_with_recovery(
        adapter.as_ref(),
        &issue,
        &ProjectFieldAssignment {
            name: lane.claim_field().into(),
            value: claim_value.clone(),
        },
        "claim_field",
    )?;
    let registry_path =
        record_manual_lane_claim_evidence(&config, &issue, lane, &claim, &claim_value, &worker)?;
    let command_name = format!("{} claim", lane.label());
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: &command_name,
                mutation_type: "claim_field",
                issue_ref: Some(&issue.identifier),
                target: Some(format!("{}={claim_value}", lane.claim_field())),
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "manual lane worker claim",
            },
        );
    }
    println!(
        "{}_claim={} issue_ref={} field={:?} worker={} run={} registry={} value={claim_value}",
        lane.label(),
        outcome.as_str(),
        issue.identifier,
        lane.claim_field(),
        worker.trim(),
        claim.run,
        registry_path.display()
    );
    Ok(())
}

pub(crate) fn validate_lane_claim_state(
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let normalized = issue.normalized_state();
    let valid = match lane {
        AgentSessionLaneArg::Main => {
            matches!(normalized.as_str(), "todo" | "in progress" | "rework")
        }
        AgentSessionLaneArg::Review => normalized == "agent review",
        AgentSessionLaneArg::Merge => normalized == "merging",
    };
    if valid {
        if matches!(lane, AgentSessionLaneArg::Main) {
            let terminal_states = config.terminal_state_set().into_iter().collect();
            if let Some(reason) = native_subissue_gate_blocker(issue, &terminal_states) {
                return Err(format!(
                    "{} claim cannot claim {}; {reason}",
                    lane.label(),
                    issue.identifier
                )
                .into());
            }
        }
        return Ok(());
    }

    Err(format!(
        "{} claim cannot claim {}; {} is currently {}",
        lane.label(),
        issue.identifier,
        issue.identifier,
        issue.state
    )
    .into())
}

fn actor_from_worker(worker: &str) -> LaneClaimActor {
    let normalized = worker.to_ascii_lowercase();
    if normalized.contains("gemini") {
        LaneClaimActor::Gemini
    } else if normalized.contains("claude") {
        LaneClaimActor::Claude
    } else if normalized.contains("human") {
        LaneClaimActor::Human
    } else {
        LaneClaimActor::Codex
    }
}

pub(crate) fn lane_claim_for_manual_worker(
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    actor: LaneClaimActor,
    source: LaneClaimSource,
    worker: &str,
    existing: Option<&str>,
) -> Result<LaneClaim, Box<dyn std::error::Error>> {
    if let Some(existing) = existing {
        if let Ok(claim) = LaneClaim::parse(existing) {
            if claim.lane == lane.claim_lane()
                && claim.issue == issue.identifier
                && claim.state == LaneClaimState::Active
            {
                if claim.worker.as_deref() == Some(worker) {
                    return Ok(claim);
                }
                return Err(format!(
                    "{} already has an active {} claim owned by {} run={}",
                    issue.identifier,
                    lane.label(),
                    claim.worker.as_deref().unwrap_or(claim.actor.as_str()),
                    claim.run
                )
                .into());
            }
        } else if !existing.trim().is_empty() {
            return Err(format!(
                "{} already has an unparseable {} claim: {existing}",
                issue.identifier,
                lane.claim_field()
            )
            .into());
        }
    }

    Ok(LaneClaim::active(
        &issue.identifier,
        lane.claim_lane(),
        actor,
        source,
        current_time_ms(),
    )
    .with_worker(worker))
}

pub(crate) fn record_manual_lane_claim_evidence(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    claim: &LaneClaim,
    claim_value: &str,
    worker: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let registry_path = session_registry_path(config);
    let now_ms = unix_timestamp_ms();
    let path = std::env::current_dir()?;
    let branch = current_git_branch(&path).ok().flatten();
    let session_name = format!("manual-{}-{}", lane.label(), safe_identifier(&claim.run));
    let record = AgentSessionRecord {
        issue_id: Some(issue.id.clone()),
        issue_identifier: Some(issue.identifier.clone()),
        issue_title: Some(issue.title.clone()),
        lane: lane.label().into(),
        run_id: Some(claim.run.clone()),
        thread: Some(claim.thread.clone()),
        session_source: Some("manual-claim".into()),
        claim_value: Some(claim_value.into()),
        actor_role: Some(claim.actor.as_str().into()),
        actor_label: Some(worker.trim().into()),
        git_author: None,
        profile_id: None,
        instance_name: None,
        worktree: path,
        branch,
        backend: "codex-app-manual".into(),
        session_name,
        pane_target: String::new(),
        prompt_artifact_path: PathBuf::new(),
        log_path: PathBuf::new(),
        attach_command: "not a tmux session; manual Codex App evidence only".into(),
        attempt: 1,
        status: SessionStatus::Recorded,
        started_at_ms: now_ms,
        updated_at_ms: now_ms,
    };
    save_session_record(&registry_path, record)?;
    Ok(registry_path)
}

pub(crate) fn timeline_claim_run(value: &str) -> Option<String> {
    LaneClaim::parse(value).ok().map(|claim| claim.run)
}

pub(crate) fn timeline_claim_actor(value: &str) -> Option<String> {
    LaneClaim::parse(value)
        .ok()
        .map(|claim| claim.actor.as_str().to_string())
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

pub(crate) fn agent_session_start(
    workflow_path: PathBuf,
    issue_ref: String,
    lane: AgentSessionLaneArg,
    run_id: Option<String>,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let run_id = run_id.ok_or("session start requires explicit --run <RUN_ID>")?;
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    let backend_spec = agent_session_backend_spec(&config, lane)?;
    preflight_canonical_checkout_for_write_mode(&config, "session start", write)?;

    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let workspace_key = agent_session_workspace_key(&config, &issue, lane)?;
    let prompt_path =
        rendered_lane_prompt_artifact_path(&config, &issue, lane, 1, &backend_spec.backend);
    let claim = matching_lane_claim_for_session(&issue, lane, &run_id)?;

    if !write {
        println!(
            "session_dry_run action=start issue={} lane={} run={} backend={} agent_command={} workspace_key={} prompt_artifact={}",
            issue.identifier,
            lane.label(),
            claim.run,
            backend_spec.backend,
            shell_quote_display(&backend_spec.command),
            workspace_key,
            prompt_path.display()
        );
        return Ok(());
    }

    let started = start_agent_session_with_claim(
        &workflow,
        &config,
        adapter.as_ref(),
        &issue,
        lane,
        &claim,
        "session start",
    )?;

    println!(
        "session_action=started issue={} lane={} run={} backend={} session={} pending_session={} workspace={} prompt_artifact={}",
        issue.identifier,
        lane.label(),
        claim.run,
        started.summary.backend,
        started.summary.session_id.as_deref().unwrap_or("n/a"),
        started.summary.pending_session,
        started.workspace_path.display(),
        started.prompt_path.display()
    );
    if let Some(attach_command) = started.summary.attach_command.as_deref() {
        println!("attach_command={attach_command}");
    }
    if let Some(log_path) = started.summary.log_path.as_ref() {
        println!("log_path={}", log_path.display());
    }
    Ok(())
}

struct AgentSessionStartResult {
    summary: AgentSummary,
    workspace_path: PathBuf,
    prompt_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSessionBackendSpec {
    pub(crate) backend: String,
    pub(crate) command: String,
}

fn start_agent_session_with_claim(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    claim: &LaneClaim,
    audit_command: &'static str,
) -> Result<AgentSessionStartResult, Box<dyn std::error::Error>> {
    let backend_spec = agent_session_backend_spec(config, lane)?;
    let workspace_key = agent_session_workspace_key(config, issue, lane)?;
    let prompt_path =
        rendered_lane_prompt_artifact_path(config, issue, lane, 1, &backend_spec.backend);
    let workspace = prepare_workspace(&config.workspace.root, &workspace_key, &config.hooks)?;
    let git_identity = apply_local_git_identity(&workspace.path, &config.identity.git)?;
    let prompt = render_prompt_with_claim(
        workflow.prompt_for_lane(lane.workflow_lane()),
        issue,
        None,
        Some(claim),
    )?;
    let backend = agent_session_backend(&backend_spec.backend)?;
    let mut prepared = backend.prepare(workspace.path.clone(), prompt, config)?;
    prepared.command = Some(backend_spec.command.clone());
    prepared
        .env
        .insert("JADE_SYMPHONY_AGENT_LANE".into(), lane.label().to_string());
    prepared.env.insert(
        "JADE_SYMPHONY_AGENT_COMMAND".into(),
        prepared.command.clone().unwrap_or_default(),
    );
    prepared.env.insert(
        "JADE_SYMPHONY_AGENT_BACKEND".into(),
        backend_spec.backend.clone(),
    );
    if backend_spec.backend == "tmux" {
        prepared.env.insert(
            "JADE_SYMPHONY_TMUX_AGENT_COMMAND".into(),
            prepared.command.clone().unwrap_or_default(),
        );
    }
    prepared.prompt_artifact_path = Some(prompt_path.clone());
    prepared.issue_id = Some(issue.id.clone());
    prepared.issue_identifier = Some(issue.identifier.clone());
    prepared.issue_title = Some(issue.title.clone());
    prepared.lane = Some(lane.label().into());
    prepared.run_id = Some(claim.run.clone());
    prepared
        .env
        .insert("JADE_SYMPHONY_RUN_ID".into(), claim.run.clone());
    prepared
        .env
        .insert("JADE_SYMPHONY_CLAIM".into(), claim.render());
    prepared.attempt = 1;
    prepared.branch_name = current_git_branch(&workspace.path).ok().flatten();

    let events = backend.run(prepared)?;
    let summary = backend.summarize(&events);
    record_agent_session_events(config, issue, lane, &summary, &events, &prompt_path)?;

    let claim_value = claim.render();
    let workpad = agent_session_workpad(AgentSessionWorkpadInput {
        issue,
        lane,
        workspace_path: &workspace.path,
        summary: &summary,
        prompt_path: &prompt_path,
        claim_value: &claim_value,
        agent_command: &backend_spec.command,
        git_identity: &git_identity,
    });
    let (mutation_type, outcome) = if lane == AgentSessionLaneArg::Main {
        let key = recovery_key(
            "session-start-workpad",
            &issue.identifier,
            &format!(
                "{}|{}|{}",
                issue.identifier,
                claim.run,
                stable_recovery_hash(&workpad)
            ),
        );
        (
            "workpad_write",
            upsert_workpad_with_recovery(adapter, &issue.identifier, Some(issue), &workpad, &key)?,
        )
    } else {
        let key = recovery_key(
            "session-start-timeline",
            &issue.identifier,
            &format!(
                "{}|{}|{}",
                issue.identifier,
                claim.run,
                stable_recovery_hash(&workpad)
            ),
        );
        (
            "timeline_comment",
            add_timeline_comment_with_recovery(
                adapter,
                &issue.identifier,
                Some(issue),
                &workpad,
                &key,
                "timeline_comment",
            )?,
        )
    };
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: audit_command,
                mutation_type,
                issue_ref: Some(&issue.identifier),
                target: summary.session_id.clone(),
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "manual lane session evidence",
            },
        );
    }

    Ok(AgentSessionStartResult {
        summary,
        workspace_path: workspace.path,
        prompt_path,
    })
}

pub(crate) fn legacy_agent_session_start(
    _workflow_path: PathBuf,
    _issue_ref: String,
    lane: AgentSessionLaneArg,
    _write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    Err(format!(
        "legacy session aliases are unavailable; use `{} claim` first, then `session start --lane {} --run <RUN_ID>`",
        lane.label(),
        lane.label()
    )
    .into())
}

pub(crate) fn matching_lane_claim_for_session(
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    run_id: &str,
) -> Result<LaneClaim, Box<dyn std::error::Error>> {
    let claim_value = project_text_field(issue, lane.claim_field()).ok_or_else(|| {
        format!(
            "session start requires an existing {} claim for {}",
            lane.claim_field(),
            issue.identifier
        )
    })?;
    let claim = LaneClaim::parse(&claim_value)?;
    if claim.lane != lane.claim_lane() {
        return Err(format!(
            "session start lane mismatch for {}; claim lane={} requested lane={}",
            issue.identifier,
            claim.lane.as_str(),
            lane.label()
        )
        .into());
    }
    if claim.issue != issue.identifier {
        return Err(format!(
            "session start issue mismatch for {}; claim points at {}",
            issue.identifier, claim.issue
        )
        .into());
    }
    if claim.run != run_id {
        return Err(format!(
            "session start run mismatch for {}; claim run={} requested run={run_id}",
            issue.identifier, claim.run
        )
        .into());
    }
    if claim.state != LaneClaimState::Active {
        return Err(format!(
            "session start requires an active claim; {} claim state={}",
            issue.identifier,
            claim.state.as_str()
        )
        .into());
    }
    if claim.worker.as_deref().unwrap_or("").trim().is_empty() {
        return Err(format!(
            "session start requires a structured worker= claim for {} run={}",
            issue.identifier, claim.run
        )
        .into());
    }
    Ok(claim)
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

fn validate_tmux_session_config(config: &RuntimeConfig) -> Result<(), Box<dyn std::error::Error>> {
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

fn agent_session_workspace_key(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
) -> Result<String, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    let base = format!("{}-{}-agent", issue.identifier, lane.label());
    Ok(profile_scoped_identifier(
        profile
            .as_ref()
            .map(|profile| profile.workspace_namespace.as_str()),
        &base,
    ))
}

pub(crate) fn rendered_lane_prompt_artifact_path(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    attempt: u32,
    backend: &str,
) -> PathBuf {
    config.observability.logs_root.join("prompts").join(format!(
        "{}-{}-attempt-{}-{}-{}.prompt.md",
        safe_identifier(&issue.identifier),
        lane.label(),
        attempt,
        safe_identifier(backend),
        current_time_ms()
    ))
}

pub(crate) fn record_agent_session_events(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    summary: &AgentSummary,
    events: &[AgentEvent],
    prompt_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    log.append(&EventRecord {
        event: "agent_session_prompt_artifact".into(),
        issue_id: Some(issue.id.clone()),
        issue_identifier: Some(issue.identifier.clone()),
        session_id: summary.session_id.clone(),
        profile_id: None,
        instance_name: None,
        actor_role: Some(config.identity.actor_role.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        git_author: config.identity.git.author(),
        tracker_mutation: None,
        message: format!(
            "lane={} prompt_artifact={}",
            lane.label(),
            prompt_path.display()
        ),
    })?;
    for event in events {
        log.append(&EventRecord {
            event: format!("agent_session_{event:?}"),
            issue_id: Some(issue.id.clone()),
            issue_identifier: Some(issue.identifier.clone()),
            session_id: summary.session_id.clone(),
            profile_id: None,
            instance_name: None,
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
            tracker_mutation: None,
            message: format!("lane={} {}", lane.label(), summary.message),
        })?;
    }
    Ok(())
}

struct AgentSessionWorkpadInput<'a> {
    issue: &'a TrackerIssue,
    lane: AgentSessionLaneArg,
    workspace_path: &'a Path,
    summary: &'a AgentSummary,
    prompt_path: &'a Path,
    claim_value: &'a str,
    agent_command: &'a str,
    git_identity: &'a GitIdentityApplyResult,
}

fn agent_session_workpad(input: AgentSessionWorkpadInput<'_>) -> String {
    let attach_command = input.summary.attach_command.as_deref().unwrap_or("n/a");
    let log_path = input
        .summary
        .log_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "n/a".into());
    let title = match input.lane {
        AgentSessionLaneArg::Main => "## Jade Symphony Workpad",
        AgentSessionLaneArg::Review => "## Jade Symphony Agent Review Run",
        AgentSessionLaneArg::Merge => "## Jade Symphony Merge Run",
    };
    let session_heading = if input.summary.backend == "tmux" {
        "### Local tmux Agent Session"
    } else {
        "### Local Agent Session"
    };
    let evidence_summary = if input.summary.backend == "tmux" {
        "tmux session, prompt artifact, log path, workspace, and claim metadata recorded."
    } else {
        "backend session, prompt artifact, workspace, and claim metadata recorded."
    };
    [
        title.to_string(),
        String::new(),
        session_heading.to_string(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", input.issue.identifier, input.issue.title),
        format!("- Lane: `{}`", input.lane.label()),
        format!(
            "- Actor role: `{}`",
            match input.lane {
                AgentSessionLaneArg::Main => "implementation_agent",
                AgentSessionLaneArg::Review => "review_agent",
                AgentSessionLaneArg::Merge => "merge_agent",
            }
        ),
        format!(
            "- Actor: `{}`",
            timeline_claim_actor(input.claim_value).unwrap_or_else(|| "not recorded".into())
        ),
        format!(
            "- Run ID: `{}`",
            timeline_claim_run(input.claim_value).unwrap_or_else(|| "not recorded".into())
        ),
        format!(
            "- Input state: `{}`",
            match input.lane {
                AgentSessionLaneArg::Main => input.issue.state.as_str(),
                AgentSessionLaneArg::Review => "Agent Review",
                AgentSessionLaneArg::Merge => "Merging",
            }
        ),
        format!(
            "- Target state after run: `{}`",
            match input.lane {
                AgentSessionLaneArg::Main => "Agent Review",
                AgentSessionLaneArg::Review => {
                    "Human Review | Merging | Rework | Need Human Input | unchanged"
                }
                AgentSessionLaneArg::Merge => "Done | Need Human Input | unchanged",
            }
        ),
        format!(
            "- Result: `{}`",
            if input.summary.pending_session {
                "session_started"
            } else {
                "session_recorded"
            }
        ),
        format!("- PR: `{}`", timeline_pr_summary(input.issue)),
        format!(
            "- Claim field: `{}` = `{}`",
            input.lane.claim_field(),
            input.claim_value
        ),
        format!("- Backend: `{}`", input.summary.backend),
        format!("- Agent command: `{}`", input.agent_command),
        format!(
            "- Session: `{}`",
            input.summary.session_id.as_deref().unwrap_or("n/a")
        ),
        format!("- Pending session: `{}`", input.summary.pending_session),
        format!("- Workspace: `{}`", input.workspace_path.display()),
        format!("- Prompt artifact: `{}`", input.prompt_path.display()),
        format!("- Session log: `{log_path}`"),
        format!("- Attach command: `{attach_command}`"),
        format!("- Git identity: `{}`", input.git_identity.summary()),
        format!("- Evidence summary: {evidence_summary}"),
        String::new(),
        input.summary.message.clone(),
    ]
    .join("\n")
}
