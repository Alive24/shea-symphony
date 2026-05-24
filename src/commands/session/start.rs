use std::path::{Path, PathBuf};

use shea_symphony::agent::AgentSummary;
use shea_symphony::config::RuntimeConfig;
use shea_symphony::event_log::{EventLog, EventRecord};
use shea_symphony::lane_claim::LaneClaim;
use shea_symphony::model::{AgentEvent, TrackerIssue};
use shea_symphony::profiles::selected_execution_profile;
use shea_symphony::tracker::{adapter_from_config, TrackerAdapter};
use shea_symphony::workflow::WorkflowDefinition;
use shea_symphony::workspace::{
    apply_local_git_identity, prepare_workspace, profile_scoped_identifier, safe_identifier,
    GitIdentityApplyResult,
};

use crate::lanes::claim::render_prompt_with_claim;
use crate::orchestration::{
    add_timeline_comment_with_recovery, append_tracker_mutation_audit, current_git_branch,
    current_gmt_timestamp, current_time_ms, preflight_canonical_checkout_for_write_mode,
    recovery_key, shell_quote_display, stable_recovery_hash, upsert_workpad_with_recovery,
    TrackerMutationAudit,
};

use super::{
    agent_session_backend, agent_session_backend_spec, matching_lane_claim_for_session,
    timeline_claim_actor, timeline_claim_run, timeline_pr_summary, AgentSessionLaneArg,
};

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
        .insert("SHEA_SYMPHONY_AGENT_LANE".into(), lane.label().to_string());
    prepared.env.insert(
        "SHEA_SYMPHONY_AGENT_COMMAND".into(),
        prepared.command.clone().unwrap_or_default(),
    );
    prepared.env.insert(
        "SHEA_SYMPHONY_AGENT_BACKEND".into(),
        backend_spec.backend.clone(),
    );
    if backend_spec.backend == "tmux" {
        prepared.env.insert(
            "SHEA_SYMPHONY_TMUX_AGENT_COMMAND".into(),
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
        .insert("SHEA_SYMPHONY_RUN_ID".into(), claim.run.clone());
    prepared
        .env
        .insert("SHEA_SYMPHONY_CLAIM".into(), claim.render());
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
    let log = EventLog::new(config.observability.logs_root.join("shea-symphony.jsonl"));
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
        AgentSessionLaneArg::Main => "## Shea Symphony Workpad",
        AgentSessionLaneArg::Review => "## Shea Symphony Agent Review Run",
        AgentSessionLaneArg::Merge => "## Shea Symphony Merge Run",
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
