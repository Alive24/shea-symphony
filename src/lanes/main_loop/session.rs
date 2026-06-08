use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use shea_symphony::agent::UsageLimitPause;
use shea_symphony::codex_app_server::BACKEND_NAME as CODEX_APP_SERVER_BACKEND_NAME;
use shea_symphony::config::RuntimeConfig;
use shea_symphony::handoff::IssueHandoffPlan;
use shea_symphony::lane_claim::LaneClaim;
use shea_symphony::model::{normalize_state, TrackerIssue};
use shea_symphony::profiles::selected_execution_profile;
use shea_symphony::runtime_state::{
    load_runtime_states, remove_runtime_state_for_issue, RuntimeIssueState, RuntimeState,
    RuntimeTransition,
};
use shea_symphony::session_registry::{
    capture_tmux_pane_tail, classify_session_record, load_session_registry, read_log_tail,
    save_session_registry, session_registry_path, unix_timestamp_ms, AgentSessionRecord,
    SessionStatus,
};
use shea_symphony::workspace::{GitIdentityApplyResult, GitIdentityApplyStatus};

use super::{
    append_runtime_supervision_event, runtime_state_issue_identifier, IssueExecutionResult,
};
use crate::lanes::main_loop::compact_evidence;
use crate::orchestration::{
    current_time_ms, DEFAULT_SESSION_STALE_AFTER_MS, DEFAULT_SESSION_STATUS_LINES,
};

const RECOVERY_ARTIFACT_CHAR_LIMIT: usize = 12_000;
const RECOVERY_EVENT_LOG_LINE_LIMIT: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainRecoveryMode {
    NativeThread,
    TranscriptReplay,
    WorktreeOnly,
}

impl MainRecoveryMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NativeThread => "native_thread",
            Self::TranscriptReplay => "transcript_replay",
            Self::WorktreeOnly => "worktree_only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MainRecoveryPlan {
    pub(crate) mode: MainRecoveryMode,
    pub(crate) app_server_resume_thread_id: Option<String>,
    pub(crate) prompt_override: Option<String>,
    pub(crate) evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MainSessionReconciliation {
    Terminal(Box<IssueExecutionResult>),
    Active {
        status: String,
        source: String,
        evidence: String,
    },
}

pub(crate) fn main_session_active_recoverable(status: &str, evidence: &str) -> bool {
    status == "stale"
        || (status == "unknown"
            && (evidence.contains("missing from session registry")
                || evidence.contains("without backend session id")
                || evidence.contains("tmux")
                || evidence.contains("unavailable")))
}

pub(crate) fn codex_app_server_resume_thread_for_state(
    config: &RuntimeConfig,
    state: &RuntimeState,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    Ok(codex_app_server_session_record_for_state(config, state)?
        .and_then(|record| record.thread.clone())
        .filter(|thread_id| !thread_id.trim().is_empty()))
}

pub(crate) fn main_recovery_plan_applicable(state: &RuntimeState) -> bool {
    state
        .backend_session_id
        .as_deref()
        .is_some_and(|session_id| !session_id.trim().is_empty())
        || state.backend_log_path.is_some()
        || state.workspace_path.is_some()
        || matches!(
            state.last_event.as_deref(),
            Some("SessionRunning" | "SessionTerminal" | "Failed" | "Completed")
        )
}

pub(crate) fn main_recovery_plan(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    state: &RuntimeState,
) -> Result<MainRecoveryPlan, Box<dyn std::error::Error>> {
    if let Some(thread_id) = codex_app_server_resume_thread_for_state(config, state)? {
        return Ok(MainRecoveryPlan {
            mode: MainRecoveryMode::NativeThread,
            app_server_resume_thread_id: Some(thread_id.clone()),
            prompt_override: None,
            evidence: format!("thread={thread_id}"),
        });
    }

    let record = codex_app_server_session_record_for_state(config, state)?;
    if let Some(prompt) = transcript_recovery_prompt(config, issue, state, record.as_ref())? {
        return Ok(MainRecoveryPlan {
            mode: MainRecoveryMode::TranscriptReplay,
            app_server_resume_thread_id: None,
            prompt_override: Some(prompt),
            evidence: "local_conversation_artifacts=present".into(),
        });
    }

    Ok(MainRecoveryPlan {
        mode: MainRecoveryMode::WorktreeOnly,
        app_server_resume_thread_id: None,
        prompt_override: Some(worktree_only_recovery_prompt(issue, state)?),
        evidence: "local_conversation_artifacts=missing".into(),
    })
}

fn codex_app_server_session_record_for_state(
    config: &RuntimeConfig,
    state: &RuntimeState,
) -> Result<Option<AgentSessionRecord>, Box<dyn std::error::Error>> {
    if state.backend != "codex" {
        return Ok(None);
    }
    let registry = load_session_registry(&session_registry_path(config))?;
    if let Some(session_id) = state.backend_session_id.as_deref() {
        if let Some(record) = registry.sessions.iter().rev().find(|record| {
            record.session_name == session_id
                && record.session_source.as_deref() == Some(CODEX_APP_SERVER_BACKEND_NAME)
        }) {
            return Ok(Some(record.clone()));
        }
    }
    let issue_identifier = runtime_state_issue_identifier(state);
    Ok(registry
        .sessions
        .iter()
        .rev()
        .find(|record| {
            record.session_source.as_deref() == Some(CODEX_APP_SERVER_BACKEND_NAME)
                && record.lane.eq_ignore_ascii_case("main")
                && record.issue_identifier.as_deref() == issue_identifier
        })
        .cloned())
}

fn transcript_recovery_prompt(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    state: &RuntimeState,
    record: Option<&AgentSessionRecord>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut sections = Vec::new();
    let mut has_conversation_artifact = false;
    for path in app_server_protocol_paths(state, record) {
        if push_readable_artifact(&mut sections, "app-server protocol", &path) {
            has_conversation_artifact = true;
            break;
        }
    }
    for path in app_server_event_paths(state, record) {
        if push_readable_artifact(&mut sections, "app-server normalized events", &path) {
            has_conversation_artifact = true;
            break;
        }
    }
    if let Some(record) = record {
        if push_readable_artifact(
            &mut sections,
            "original prompt artifact",
            &record.prompt_artifact_path,
        ) {
            has_conversation_artifact = true;
        }
    }

    if !has_conversation_artifact {
        return Ok(None);
    }

    if let Some(lines) = issue_event_log_excerpt(config, issue, state, record) {
        sections.push(("Shea event log records".into(), lines));
    }

    if let Some(record) = record {
        sections.push((
            "session registry metadata".into(),
            bounded_text(
                &serde_json::to_string_pretty(record)?,
                RECOVERY_ARTIFACT_CHAR_LIMIT,
            ),
        ));
    }

    Ok(Some(recovery_prompt(
        "transcript_replay",
        issue,
        state,
        sections,
    )?))
}

fn worktree_only_recovery_prompt(
    issue: &TrackerIssue,
    state: &RuntimeState,
) -> Result<String, Box<dyn std::error::Error>> {
    recovery_prompt("worktree_only", issue, state, Vec::new())
}

fn recovery_prompt(
    mode: &str,
    issue: &TrackerIssue,
    state: &RuntimeState,
    sections: Vec<(String, String)>,
) -> Result<String, Box<dyn std::error::Error>> {
    let workspace = state
        .workspace_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unknown".into());
    let branch = state.branch_name.as_deref().unwrap_or("unknown");
    let status = git_output(state.workspace_path.as_deref(), &["status", "--short"]);
    let diff_stat = git_output(state.workspace_path.as_deref(), &["diff", "--stat"]);
    let mut prompt = format!(
        "# Shea Symphony Main Recovery\n\n\
Recovery mode: `{mode}`\n\n\
Continue issue {identifier}: {title}\n\n\
You are starting a new turn to recover an interrupted Shea Symphony Main Agent run. \
Preserve the dirty worktree. Do not reset, clean, delete, or discard local files. \
Inspect the current worktree first, then continue the implementation, verification, and normal handoff evidence from the latest available state.\n\n\
## Runtime Context\n\n\
- Issue state: {state_name}\n\
- Workspace: {workspace}\n\
- Branch: {branch}\n\
- Previous backend session: {session}\n\
- Previous runtime event: {event}\n\
- Attempt: {attempt}\n\n\
## Current Worktree Status\n\n```text\n{status}\n```\n\n\
## Current Diff Stat\n\n```text\n{diff_stat}\n```\n",
        identifier = issue.identifier,
        title = issue.title,
        state_name = issue.state,
        session = state.backend_session_id.as_deref().unwrap_or("none"),
        event = state.last_event.as_deref().unwrap_or("unknown"),
        attempt = state.attempt_count,
    );
    for (title, body) in sections {
        prompt.push_str(&format!("\n## {title}\n\n```text\n{body}\n```\n"));
    }
    prompt.push_str(
        "\n## Recovery Instructions\n\n\
1. Reconstruct the prior state from the context above.\n\
2. Inspect the worktree before editing.\n\
3. Continue the issue from the preserved local changes.\n\
4. Run focused verification appropriate to the touched code.\n\
5. Produce Shea Symphony handoff/readback evidence when ready.\n",
    );
    Ok(prompt)
}

fn app_server_event_paths(
    state: &RuntimeState,
    record: Option<&AgentSessionRecord>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = state.backend_log_path.clone() {
        paths.push(path);
    }
    if let Some(record) = record {
        paths.push(record.log_path.clone());
    }
    dedupe_paths(paths)
}

fn app_server_protocol_paths(
    state: &RuntimeState,
    record: Option<&AgentSessionRecord>,
) -> Vec<PathBuf> {
    app_server_event_paths(state, record)
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            name.strip_suffix(".events.json")
                .map(|base| path.with_file_name(format!("{base}.protocol.jsonl")))
        })
        .collect()
}

fn push_readable_artifact(sections: &mut Vec<(String, String)>, title: &str, path: &Path) -> bool {
    if let Some(text) = read_bounded_artifact(path) {
        sections.push((
            format!("{title}: {}", path.display()),
            bounded_text(&text, RECOVERY_ARTIFACT_CHAR_LIMIT),
        ));
        return true;
    }
    false
}

fn read_bounded_artifact(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .filter(|content| !content.trim().is_empty())
        .map(|content| bounded_text(&content, RECOVERY_ARTIFACT_CHAR_LIMIT))
}

fn issue_event_log_excerpt(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    state: &RuntimeState,
    record: Option<&AgentSessionRecord>,
) -> Option<String> {
    let path = config.observability.logs_root.join("shea-symphony.jsonl");
    let content = fs::read_to_string(path).ok()?;
    let session_id = state
        .backend_session_id
        .as_deref()
        .or_else(|| record.map(|record| record.session_name.as_str()));
    let mut lines = content
        .lines()
        .filter(|line| {
            line.contains(&issue.identifier)
                || session_id.is_some_and(|session_id| line.contains(session_id))
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    if lines.len() > RECOVERY_EVENT_LOG_LINE_LIMIT {
        lines = lines.split_off(lines.len().saturating_sub(RECOVERY_EVENT_LOG_LINE_LIMIT));
    }
    Some(bounded_text(
        &lines.join("\n"),
        RECOVERY_ARTIFACT_CHAR_LIMIT,
    ))
}

fn git_output(workspace: Option<&Path>, args: &[&str]) -> String {
    let Some(workspace) = workspace else {
        return "workspace path unavailable".into();
    };
    match Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(args)
        .output()
    {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                "<empty>".into()
            } else {
                bounded_text(&text, RECOVERY_ARTIFACT_CHAR_LIMIT)
            }
        }
        Ok(output) => format!(
            "git {:?} failed: {}",
            args,
            compact_evidence(&String::from_utf8_lossy(&output.stderr))
        ),
        Err(error) => format!("git {:?} failed: {error}", args),
    }
}

fn bounded_text(text: &str, max_chars: usize) -> String {
    let mut output = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        output.push_str("\n...[truncated]");
    }
    output
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique = Vec::new();
    for path in paths {
        if !unique.iter().any(|existing| existing == &path) {
            unique.push(path);
        }
    }
    unique
}

pub(crate) fn run_loop_runtime_state_for_issue(
    existing: Option<&RuntimeState>,
    issue: &TrackerIssue,
    config: &RuntimeConfig,
    event: &str,
    claim: &LaneClaim,
) -> RuntimeState {
    if event == "Resumed" {
        if let Some(existing) = existing
            .filter(|state| {
                runtime_state_issue_identifier(state) == Some(issue.identifier.as_str())
            })
            .filter(|state| {
                state.last_event.as_deref() == Some("SessionTerminal")
                    || (state.backend == "codex" && state.backend_session_id.is_some())
            })
        {
            let mut state = existing.clone();
            state.run_id.get_or_insert_with(|| claim.run.clone());
            if state.last_event.as_deref() != Some("SessionTerminal") {
                state.attempt_count = state.attempt_count.saturating_add(1);
            }
            return state;
        }
    }

    let profile = selected_execution_profile(&config.profiles).ok().flatten();
    let mut state = RuntimeState::active(
        RuntimeIssueState {
            id: issue.id.clone(),
            identifier: issue.identifier.clone(),
        },
        &config.backend.kind,
    );
    state.attempt_count = next_runtime_attempt_count(existing, &issue.identifier);
    state.branch_name = issue.branch_name.clone();
    state.lane = Some("main".into());
    state.run_id = Some(claim.run.clone());
    state.profile_id = profile.as_ref().map(|profile| profile.profile_id.clone());
    state.instance_name = profile
        .as_ref()
        .map(|profile| profile.instance_name.clone());
    state.actor_role = Some(config.identity.actor_role.clone());
    state.actor_label = Some(config.identity.actor_label.clone());
    state.git_author = config.identity.git.author();
    state.last_event = Some(event.into());
    state
}

fn next_runtime_attempt_count(existing: Option<&RuntimeState>, issue_identifier: &str) -> u32 {
    existing
        .and_then(|state| {
            state
                .active_issue
                .as_ref()
                .filter(|issue| issue.identifier == issue_identifier)
                .map(|_| state.attempt_count.saturating_add(1))
        })
        .unwrap_or(1)
}

pub(crate) fn run_loop_runtime_state_with_result(
    mut state: RuntimeState,
    result: &IssueExecutionResult,
) -> RuntimeState {
    state.workspace_path = Some(result.workspace_path.clone());
    state.backend = result.backend.clone();
    state.backend_session_id = result.session_id.clone();
    state.run_id = result.run_id.clone();
    state.backend_log_path = result.backend_log_path.clone();
    state.backend_attach_command = result.backend_attach_command.clone();
    state.profile_id = result.profile_id.clone();
    state.instance_name = result.instance_name.clone();
    state.actor_role = Some(result.actor_role.clone());
    state.actor_label = Some(result.actor_label.clone());
    state.git_author = result.git_author.clone();
    state.last_event = Some(if result.pending_session {
        "SessionRunning".into()
    } else if result.success {
        "Completed".into()
    } else {
        "Failed".into()
    });
    state
}

pub(crate) fn run_loop_runtime_state_with_transition(
    mut state: RuntimeState,
    from: Option<String>,
    to: &str,
    reason: &str,
) -> RuntimeState {
    state.last_transition = Some(RuntimeTransition {
        from,
        to: to.into(),
        reason: reason.into(),
    });
    state
}

pub(crate) fn reconcile_pending_main_session(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    handoff: &IssueHandoffPlan,
    state: &RuntimeState,
) -> Result<Option<MainSessionReconciliation>, Box<dyn std::error::Error>> {
    let Some(active_issue) = state.active_issue.as_ref() else {
        return Ok(None);
    };
    if active_issue.identifier != issue.identifier {
        return Ok(None);
    }
    if !matches!(
        state.last_event.as_deref(),
        Some("SessionRunning" | "SessionTerminal")
    ) {
        return Ok(None);
    }

    let Some(session_id) = state.backend_session_id.as_deref() else {
        return Ok(Some(MainSessionReconciliation::Active {
            status: "unknown".into(),
            source: "runtime".into(),
            evidence: "runtime state records SessionRunning without backend session id".into(),
        }));
    };

    let registry = load_session_registry(&session_registry_path(config))?;
    let Some(record) = registry
        .sessions
        .iter()
        .rev()
        .find(|record| record.session_name == session_id)
    else {
        return Ok(Some(MainSessionReconciliation::Active {
            status: "unknown".into(),
            source: "runtime".into(),
            evidence: format!("runtime session {session_id} is missing from session registry"),
        }));
    };

    let is_tmux_session = record.backend == "tmux";
    let pane_tail = if is_tmux_session {
        match capture_tmux_pane_tail(
            &config.tmux.command,
            &record.pane_target,
            DEFAULT_SESSION_STATUS_LINES,
        ) {
            Ok(tail) => Some(tail),
            Err(error) => {
                return Ok(Some(MainSessionReconciliation::Active {
                    status: "unknown".into(),
                    source: "tmux".into(),
                    evidence: format!(
                        "tmux pane unavailable for session {session_id}: {}",
                        compact_evidence(&error)
                    ),
                }))
            }
        }
    } else {
        None
    };
    let log_tail = if is_tmux_session {
        read_log_tail(&record.log_path, DEFAULT_SESSION_STATUS_LINES)?
    } else {
        None
    };
    let probe = classify_session_record(
        record,
        pane_tail.as_deref(),
        log_tail.as_deref(),
        unix_timestamp_ms(),
        DEFAULT_SESSION_STALE_AFTER_MS,
    );

    match probe.status {
        SessionStatus::Completed => Ok(Some(MainSessionReconciliation::Terminal(Box::new(
            result_from_reconciled_main_session(
                config,
                handoff,
                state,
                record,
                true,
                None,
                probe.evidence,
            ),
        )))),
        SessionStatus::Failed => Ok(Some(MainSessionReconciliation::Terminal(Box::new(
            result_from_reconciled_main_session(
                config,
                handoff,
                state,
                record,
                false,
                None,
                format!("main session failed: {}", probe.evidence),
            ),
        )))),
        SessionStatus::UsageLimited => Ok(Some(MainSessionReconciliation::Terminal(Box::new(
            result_from_reconciled_main_session(
                config,
                handoff,
                state,
                record,
                false,
                Some(UsageLimitPause {
                    classifier: "usage_limit".into(),
                    evidence: probe.evidence.clone(),
                }),
                format!("main session usage-limited: {}", probe.evidence),
            ),
        )))),
        _ => Ok(Some(MainSessionReconciliation::Active {
            status: probe.status.as_str().into(),
            source: probe.source.as_str().into(),
            evidence: probe.evidence,
        })),
    }
}

pub(crate) fn reconcile_main_handoff_runtime_state(
    config: &RuntimeConfig,
    issue_ref: &str,
    target_state: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if normalize_state(target_state) != "agent_review" {
        return Ok(());
    }

    let now_ms = current_time_ms();
    let registry_path = session_registry_path(config);
    let mut registry = load_session_registry(&registry_path)?;
    let mut completed_sessions = 0usize;

    for record in &mut registry.sessions {
        if record.lane == "main"
            && record
                .issue_identifier
                .as_deref()
                .is_some_and(|identifier| issue_refs_match_local(identifier, issue_ref))
            && !matches!(
                record.status,
                SessionStatus::Completed | SessionStatus::Recorded | SessionStatus::Failed
            )
        {
            record.status = SessionStatus::Completed;
            record.updated_at_ms = now_ms;
            completed_sessions += 1;
        }
    }

    if completed_sessions > 0 {
        save_session_registry(&registry_path, &registry)?;
    }

    let runtime_state = load_runtime_states(config)?.into_iter().find(|state| {
        state
            .active_issue
            .as_ref()
            .is_some_and(|issue| issue_refs_match_local(&issue.identifier, issue_ref))
            && state.lane.as_deref().is_none_or(|lane| lane == "main")
    });
    let runtime_matches_main_issue = runtime_state.is_some();
    if runtime_matches_main_issue {
        remove_runtime_state_for_issue(config, issue_ref)?;
    }

    if completed_sessions > 0 || runtime_matches_main_issue {
        append_runtime_supervision_event(
            config,
            runtime_state.as_ref(),
            "MainHandoffRuntimeReconciled",
            &format!(
                "issue={issue_ref} target_state=agent_review sessions_completed={completed_sessions} runtime_cleared={runtime_matches_main_issue}"
            ),
        )?;
        println!(
            "main_handoff_runtime_reconcile issue={issue_ref} sessions_completed={completed_sessions} runtime_cleared={runtime_matches_main_issue}"
        );
    }

    Ok(())
}

fn issue_refs_match_local(left: &str, right: &str) -> bool {
    normalize_issue_ref_local(left) == normalize_issue_ref_local(right)
}

fn normalize_issue_ref_local(value: &str) -> String {
    value.trim().trim_start_matches('#').to_string()
}

fn result_from_reconciled_main_session(
    config: &RuntimeConfig,
    handoff: &IssueHandoffPlan,
    state: &RuntimeState,
    record: &AgentSessionRecord,
    success: bool,
    usage_limit_pause: Option<UsageLimitPause>,
    message: String,
) -> IssueExecutionResult {
    IssueExecutionResult {
        workspace_path: state
            .workspace_path
            .clone()
            .unwrap_or_else(|| handoff.workspace_path.clone()),
        backend: state.backend.clone(),
        profile_id: state.profile_id.clone(),
        instance_name: state.instance_name.clone(),
        success,
        pending_session: false,
        session_id: state
            .backend_session_id
            .clone()
            .or_else(|| Some(record.session_name.clone())),
        run_id: state.run_id.clone().or_else(|| record.run_id.clone()),
        backend_log_path: state
            .backend_log_path
            .clone()
            .or_else(|| Some(record.log_path.clone())),
        backend_attach_command: state
            .backend_attach_command
            .clone()
            .or_else(|| Some(record.attach_command.clone())),
        message,
        usage_limit_pause,
        prompt_artifact_path: Some(record.prompt_artifact_path.clone()),
        actor_role: state
            .actor_role
            .clone()
            .unwrap_or_else(|| config.identity.actor_role.clone()),
        actor_label: state
            .actor_label
            .clone()
            .unwrap_or_else(|| config.identity.actor_label.clone()),
        git_author: state
            .git_author
            .clone()
            .or_else(|| config.identity.git.author()),
        git_identity: GitIdentityApplyResult {
            status: GitIdentityApplyStatus::NotConfigured,
            author: state.git_author.clone(),
            applied_keys: Vec::new(),
        },
        live_handoff: None,
        handoff_verification: None,
    }
}
