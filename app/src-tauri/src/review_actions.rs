//! Explicit single-Issue Legacy Review actions. No lane is launched on mount.
use crate::{
    cli::{run_shea_read_for_workspace, workspace_runtime_config, CommandSummary},
    workspace::{WorkspaceManager, WorkspaceProfile},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::State;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    Once,
    Recover,
}

impl ReviewAction {
    fn command(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Recover => "recover",
        }
    }
}

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionState {
    pub running: bool,
    pub issue: Option<String>,
    pub action: Option<ReviewAction>,
    pub workspace: Option<String>,
    pub result: Option<CommandSummary>,
}

struct PreparedAction {
    token: String,
    issue: String,
    action: ReviewAction,
    workspace: WorkspaceProfile,
    fingerprint: String,
    created: Instant,
}

#[derive(Default)]
struct Inner {
    preparing: bool,
    sequence: u64,
    prepared: Option<PreparedAction>,
    state: ActionState,
}

#[derive(Clone, Default)]
pub struct ReviewActionManager {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionPreview {
    token: String,
    issue: String,
    action: ReviewAction,
    workspace: String,
    summary: CommandSummary,
}

fn issue_identifier(issue: &str) -> Result<String, String> {
    let number = issue.strip_prefix('#').unwrap_or(issue);
    if number.is_empty()
        || number.len() > 12
        || !number.bytes().all(|c| c.is_ascii_digit())
        || number.starts_with('0')
    {
        return Err("Select a positive GitHub Issue number.".into());
    }
    Ok(format!("#{number}"))
}

fn action_args(
    workspace: &WorkspaceProfile,
    issue: &str,
    action: ReviewAction,
    write: bool,
) -> Vec<String> {
    vec![
        "review".into(),
        action.command().into(),
        workspace.workflow_path.clone(),
        issue.into(),
        if write { "--write" } else { "--dry-run" }.into(),
    ]
}

fn configuration_fingerprint(workspace: &WorkspaceProfile) -> Result<String, String> {
    let config = workspace_runtime_config(workspace)?;
    let mut digest = Sha256::new();
    for path in [
        workspace.workflow_file_path(),
        config.runtime_profile.path,
        workspace.target_path().join(".shea/app-profile.json"),
        workspace.target_path().join(".shea/app-profile.local.json"),
    ] {
        digest.update(path.to_string_lossy().as_bytes());
        match fs::read(&path) {
            Ok(bytes) => {
                digest.update([1]);
                digest.update(bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => digest.update([0]),
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(hex::encode(digest.finalize()))
}

fn check_readiness(workspace: &WorkspaceProfile) -> Result<Value, String> {
    let config = workspace_runtime_config(workspace)?;
    let resolved = shea_symphony::runtime_profile::resolve_runtime_readiness(
        &config.runtime_profile,
        &config.tracker,
        &workspace.target_path(),
    )
    .map_err(|error| error.to_string())?;
    for relative in [
        ".shea/prompts/need-to-clarify-handoff.md",
        ".shea/prompts/need-human-input-handoff.md",
        ".shea/prompts/human-review-handoff.md",
    ] {
        let path = workspace.target_path().join(relative);
        let canonical = fs::canonicalize(&path)
            .map_err(|_| format!("Missing App handoff prompt: {relative}"))?;
        if !canonical.starts_with(workspace.target_path())
            || fs::read_to_string(canonical)
                .map_err(|error| error.to_string())?
                .trim()
                .is_empty()
        {
            return Err(format!("Invalid App handoff prompt: {relative}"));
        }
    }
    let command = crate::cli::shea_command_spec(&["--runtime-info"], workspace)?;
    let identity = run_shea_read_for_workspace(&["--runtime-info".into()], workspace);
    if !identity.summary.ok {
        return Err(identity.summary.stderr);
    }
    let identity: Value = serde_json::from_str(&identity.stdout)
        .map_err(|_| "Runtime did not return a structured identity.")?;
    if identity["binary_role"] != "legacy_cli" || identity["compatibility"] != "shea-legacy-cli-v1"
    {
        return Err("App integration requires a marked Legacy CLI.".into());
    }
    let help = run_shea_read_for_workspace(
        &["review".into(), "recover".into(), "--help".into()],
        workspace,
    );
    if !help.summary.ok {
        return Err("Selected Legacy CLI does not support guarded Review recovery.".into());
    }
    Ok(
        json!({"ready":true,"scope":"local App resources and runtime", "workflowPath":workspace.workflow_path,
        "runtime":identity,"executable":command.program,"profile":resolved.report,
        "trackerReadinessVerified":false,"laneStarted":false}),
    )
}

#[tauri::command]
pub async fn get_app_readiness(workspace: State<'_, WorkspaceManager>) -> Result<Value, String> {
    let workspace = workspace.current();
    tauri::async_runtime::spawn_blocking(move || check_readiness(&workspace))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn get_issue_review_status(
    workspace: State<'_, WorkspaceManager>,
    issue: String,
) -> Result<Value, String> {
    let issue = issue_identifier(&issue)?;
    let workspace = workspace.current();
    tauri::async_runtime::spawn_blocking(move || {
        let args = vec![
            "review".into(),
            "status".into(),
            workspace.workflow_path.clone(),
            "--issue".into(),
            issue,
            "--json".into(),
        ];
        let run = run_shea_read_for_workspace(&args, &workspace);
        if !run.summary.ok {
            return Err(run.summary.stderr);
        }
        serde_json::from_str(&run.stdout)
            .map_err(|_| "Review status did not return valid JSON.".into())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn get_review_action_state(
    manager: State<'_, ReviewActionManager>,
) -> Result<ActionState, String> {
    Ok(manager
        .inner
        .lock()
        .map_err(|error| error.to_string())?
        .state
        .clone())
}

#[tauri::command]
pub async fn prepare_review_action(
    manager: State<'_, ReviewActionManager>,
    workspace: State<'_, WorkspaceManager>,
    issue: String,
    action: ReviewAction,
) -> Result<ActionPreview, String> {
    let issue = issue_identifier(&issue)?;
    let workspace = workspace.current();
    let manager = manager.inner().clone();
    {
        let mut inner = manager.inner.lock().map_err(|error| error.to_string())?;
        if inner.state.running || inner.preparing {
            return Err("Another Review operation is active in this App.".into());
        }
        inner.preparing = true;
        inner.prepared = None;
    }
    let preparation = tauri::async_runtime::spawn_blocking(move || {
        check_readiness(&workspace)?;
        let fingerprint = configuration_fingerprint(&workspace)?;
        let summary = run_shea_read_for_workspace(
            &action_args(&workspace, &issue, action, false),
            &workspace,
        )
        .summary;
        if !summary.ok {
            return Err(summary.stderr);
        }
        Ok((workspace, issue, fingerprint, summary))
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(|result| result);
    let mut inner = manager.inner.lock().map_err(|error| error.to_string())?;
    inner.preparing = false;
    let (workspace, issue, fingerprint, summary) = preparation?;
    inner.sequence += 1;
    let token = format!(
        "{}-{}-{}",
        std::process::id(),
        crate::cli::now_ms(),
        inner.sequence
    );
    inner.prepared = Some(PreparedAction {
        token: token.clone(),
        issue: issue.clone(),
        action,
        workspace: workspace.clone(),
        fingerprint,
        created: Instant::now(),
    });
    Ok(ActionPreview {
        token,
        issue,
        action,
        workspace: workspace.target_root,
        summary,
    })
}

fn take_prepared(
    inner: &mut Inner,
    workspace: &WorkspaceProfile,
    token: &str,
) -> Result<PreparedAction, String> {
    if inner.state.running || inner.preparing {
        return Err("Another Review operation is active in this App.".into());
    }
    let prepared = inner
        .prepared
        .take()
        .ok_or("Prepare the Review operation before confirming it.")?;
    if prepared.token != token
        || prepared.created.elapsed() > Duration::from_secs(300)
        || &prepared.workspace != workspace
    {
        return Err("Review preview changed or expired. Prepare again.".into());
    }
    if prepared.fingerprint != configuration_fingerprint(workspace)? {
        return Err("Workspace configuration changed. Prepare again.".into());
    }
    Ok(prepared)
}

#[tauri::command]
pub fn execute_review_action(
    manager: State<'_, ReviewActionManager>,
    workspace: State<'_, WorkspaceManager>,
    token: String,
) -> Result<ActionState, String> {
    let workspace = workspace.current();
    let mut inner = manager.inner.lock().map_err(|error| error.to_string())?;
    let prepared = take_prepared(&mut inner, &workspace, &token)?;
    inner.state = ActionState {
        running: true,
        issue: Some(prepared.issue.clone()),
        action: Some(prepared.action),
        workspace: Some(workspace.target_root.clone()),
        result: None,
    };
    let state = inner.state.clone();
    let manager = manager.inner().clone();
    std::thread::spawn(move || {
        // The CLI owns claims, exclusion, freshness, durable evidence and state-last routing.
        let result = run_shea_read_for_workspace(
            &action_args(&workspace, &prepared.issue, prepared.action, true),
            &workspace,
        )
        .summary;
        if let Ok(mut inner) = manager.inner.lock() {
            inner.state.running = false;
            inner.state.result = Some(result);
        }
    });
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn issue_input_cannot_become_an_option_or_cross_repository_target() {
        assert_eq!(issue_identifier("#5").unwrap(), "#5");
        for issue in [
            "--write",
            "https://github.com/a/b/issues/5",
            "a/b#5",
            "#0",
            "#5 --write",
            "",
        ] {
            assert!(issue_identifier(issue).is_err(), "{issue}");
        }
    }
    #[test]
    fn preview_never_requests_write_and_recover_never_dispatches_once() {
        let workspace = WorkspaceProfile::self_targeted(std::env::temp_dir());
        let preview = action_args(&workspace, "#5", ReviewAction::Once, false);
        assert_eq!(preview.last().unwrap(), "--dry-run");
        assert!(!preview.iter().any(|arg| arg == "--write"));
        let recover = action_args(&workspace, "#5", ReviewAction::Recover, true);
        assert_eq!(recover[1], "recover");
        assert_eq!(recover.last().unwrap(), "--write");
    }
    #[test]
    fn unprepared_and_expired_actions_refuse_execution() {
        let workspace = WorkspaceProfile::self_targeted(std::env::temp_dir());
        let mut inner = Inner::default();
        assert!(take_prepared(&mut inner, &workspace, "unknown").is_err());
        inner.prepared = Some(PreparedAction {
            token: "old".into(),
            issue: "#5".into(),
            action: ReviewAction::Once,
            workspace: workspace.clone(),
            fingerprint: String::new(),
            created: Instant::now() - Duration::from_secs(301),
        });
        assert!(take_prepared(&mut inner, &workspace, "old").is_err());
        assert!(inner.prepared.is_none());
    }
}
