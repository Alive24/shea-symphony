use std::path::PathBuf;

use jade_symphony::agent::{
    backend_from_config, persist_prompt_artifact, usage_limit_pause_from_events, UsageLimitPause,
};
use jade_symphony::config::RuntimeConfig;
use jade_symphony::event_log::{EventLog, EventRecord};
use jade_symphony::lane_claim::LaneClaim;
use jade_symphony::model::TrackerIssue;
use jade_symphony::profiles::selected_execution_profile;
use jade_symphony::progress::run_with_progress_heartbeat;
use jade_symphony::workflow::{AgentLane, WorkflowDefinition};
use jade_symphony::workspace::{
    apply_local_git_identity, prepare_workspace, profile_scoped_identifier, run_after_run,
    run_before_run, safe_identifier, GitIdentityApplyResult, Workspace,
};

use crate::{
    current_git_branch, current_time_ms, progress_spec_with_event_log, render_prompt_with_claim,
    CODEX_APP_SERVER_HANDOFF_BOUNDARY,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssueExecutionResult {
    pub(crate) workspace_path: PathBuf,
    pub(crate) backend: String,
    pub(crate) profile_id: Option<String>,
    pub(crate) instance_name: Option<String>,
    pub(crate) success: bool,
    pub(crate) pending_session: bool,
    pub(crate) session_id: Option<String>,
    pub(crate) run_id: Option<String>,
    pub(crate) backend_log_path: Option<PathBuf>,
    pub(crate) backend_attach_command: Option<String>,
    pub(crate) message: String,
    pub(crate) usage_limit_pause: Option<UsageLimitPause>,
    pub(crate) prompt_artifact_path: Option<PathBuf>,
    pub(crate) actor_role: String,
    pub(crate) actor_label: String,
    pub(crate) git_author: Option<String>,
    pub(crate) git_identity: GitIdentityApplyResult,
    pub(crate) live_handoff: Option<crate::RunLoopLiveHandoff>,
    pub(crate) handoff_verification: Option<String>,
}

pub(crate) fn execute_issue_once(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<IssueExecutionResult, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    let workspace_identifier = profile_scoped_identifier(
        profile
            .as_ref()
            .map(|profile| profile.workspace_namespace.as_str()),
        &issue.identifier,
    );
    execute_issue_once_with_workspace_key(workflow, config, issue, &workspace_identifier, 1, None)
}

pub(crate) fn execute_issue_once_with_workspace_key(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    workspace_key: &str,
    attempt: u32,
    claim: Option<&LaneClaim>,
) -> Result<IssueExecutionResult, Box<dyn std::error::Error>> {
    let workspace = prepare_workspace(&config.workspace.root, workspace_key, &config.hooks)?;
    execute_issue_once_in_workspace(workflow, config, issue, workspace, attempt, claim)
}

fn execute_issue_once_in_workspace(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    workspace: Workspace,
    attempt: u32,
    claim: Option<&LaneClaim>,
) -> Result<IssueExecutionResult, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    let git_identity = apply_local_git_identity(&workspace.path, &config.identity.git)?;
    run_before_run(&workspace.path, &config.hooks)?;

    let mut prompt = render_prompt_with_claim(
        workflow.prompt_for_lane(AgentLane::MainAgent),
        issue,
        None,
        claim,
    )?;
    if config.backend.kind == "codex" && config.codex.command.contains("app-server") {
        prompt.push_str(CODEX_APP_SERVER_HANDOFF_BOUNDARY);
    }
    let backend = backend_from_config(config);
    let mut prepared = backend.prepare(workspace.path.clone(), prompt, config)?;
    prepared.prompt_artifact_path = Some(rendered_prompt_artifact_path(
        config,
        issue,
        prepared.backend.as_str(),
        attempt,
    ));
    prepared.issue_id = Some(issue.id.clone());
    prepared.issue_identifier = Some(issue.identifier.clone());
    prepared.issue_title = Some(issue.title.clone());
    prepared.lane = Some("main".into());
    if let Some(claim) = claim {
        prepared.run_id = Some(claim.run.clone());
        prepared
            .env
            .insert("JADE_SYMPHONY_RUN_ID".into(), claim.run.clone());
        prepared
            .env
            .insert("JADE_SYMPHONY_CLAIM".into(), claim.render());
    }
    prepared.attempt = attempt;
    prepared.branch_name = current_git_branch(&workspace.path).ok().flatten();
    let prompt_artifact_path = persist_prompt_artifact(&prepared)?;
    let mut backend_wait = progress_spec_with_event_log(config, "main_backend")
        .issue(issue.identifier.clone())
        .backend(prepared.backend.clone())
        .next("waiting_for_child");
    if let Some(path) = &prepared.prompt_artifact_path {
        backend_wait = backend_wait.artifact(path.display().to_string());
    }
    let events = run_with_progress_heartbeat(backend_wait, || backend.run(prepared))?;
    let summary = backend.summarize(&events);
    let usage_limit_pause = usage_limit_pause_from_events(&events);
    run_after_run(&workspace.path, &config.hooks);

    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    log.append(&EventRecord {
        event: "prompt_artifact".into(),
        issue_id: Some(issue.id.clone()),
        issue_identifier: Some(issue.identifier.clone()),
        session_id: summary.session_id.clone(),
        profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
        instance_name: profile
            .as_ref()
            .map(|profile| profile.instance_name.clone()),
        actor_role: Some(config.identity.actor_role.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        git_author: config.identity.git.author(),
        tracker_mutation: None,
        message: format!("prompt_artifact={}", prompt_artifact_path.display()),
    })?;
    for event in &events {
        log.append(&EventRecord {
            event: format!("{event:?}"),
            issue_id: Some(issue.id.clone()),
            issue_identifier: Some(issue.identifier.clone()),
            session_id: summary.session_id.clone(),
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
            tracker_mutation: None,
            message: summary.message.clone(),
        })?;
    }

    Ok(IssueExecutionResult {
        workspace_path: workspace.path,
        backend: summary.backend,
        profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
        instance_name: profile
            .as_ref()
            .map(|profile| profile.instance_name.clone()),
        success: summary.success,
        pending_session: summary.pending_session,
        session_id: summary.session_id,
        run_id: claim.map(|claim| claim.run.clone()),
        backend_log_path: summary.log_path,
        backend_attach_command: summary.attach_command,
        message: summary.message,
        usage_limit_pause,
        prompt_artifact_path: Some(prompt_artifact_path),
        actor_role: config.identity.actor_role.clone(),
        actor_label: config.identity.actor_label.clone(),
        git_author: config.identity.git.author(),
        git_identity,
        live_handoff: None,
        handoff_verification: None,
    })
}

fn rendered_prompt_artifact_path(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    backend: &str,
    attempt: u32,
) -> PathBuf {
    config.observability.logs_root.join("prompts").join(format!(
        "{}-attempt-{}-{}-{}.prompt.md",
        safe_identifier(&issue.identifier),
        attempt,
        safe_identifier(backend),
        current_time_ms()
    ))
}
