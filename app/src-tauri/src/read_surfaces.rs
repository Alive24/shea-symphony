use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cli::{
    command_summary_value, parse_json_output, pending_result, run_shea_read_for_workspace,
    shea_command_for_workspace, timestamp_iso_like, CommandRun,
};
use crate::target_context::TargetContext;
use crate::workspace::{WorkspaceManager, WorkspaceProfile};

const DEFAULT_PROJECT_RATE_LIMIT_COOLDOWN_MS: u128 = 10 * 60 * 1000;
const PROJECT_READ_SURFACES: &[&str] = &["autopilot", "doctor", "review", "githubQueue"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewOptions {
    force: Option<bool>,
    scope: Option<String>,
}

#[tauri::command]
pub async fn get_runtime_snapshot(
    workspace: tauri::State<'_, WorkspaceManager>,
) -> Result<serde_json::Value, String> {
    let workspace_profile = workspace.current();
    tauri::async_runtime::spawn_blocking(move || {
        let output = shea_command_for_workspace(
            &[
                "status",
                "show",
                workspace_profile.workflow_path.as_str(),
                "--json",
            ],
            &workspace_profile,
        )
        .map_err(|error| format!("failed to resolve Shea runtime: {error}"))?
        .output()
        .map_err(|error| format!("failed to run status snapshot: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("invalid status JSON: {error}"))
    })
    .await
    .map_err(|error| format!("runtime snapshot task failed: {error}"))?
}

#[tauri::command]
pub async fn get_operator_overview(
    workspace: tauri::State<'_, WorkspaceManager>,
    options: Option<OverviewOptions>,
) -> Result<Value, String> {
    let scope = options
        .as_ref()
        .and_then(|options| options.scope.as_deref())
        .unwrap_or("full")
        .to_string();
    let workspace_profile = workspace.current();
    tauri::async_runtime::spawn_blocking(move || {
        build_operator_overview(&scope, &workspace_profile)
    })
    .await
    .map_err(|error| format!("operator overview task failed: {error}"))?
}

#[tauri::command]
pub async fn get_read_surface(
    workspace: tauri::State<'_, WorkspaceManager>,
    name: String,
    _force: Option<bool>,
    allow_project_fallback: Option<bool>,
) -> Result<Value, String> {
    let workspace_profile = workspace.current();
    tauri::async_runtime::spawn_blocking(move || {
        build_read_surface(
            &name,
            allow_project_fallback.unwrap_or(false),
            &workspace_profile,
        )
    })
    .await
    .map_err(|error| format!("read surface task failed: {error}"))?
}

#[tauri::command]
pub async fn get_codex_transcript(
    workspace: tauri::State<'_, WorkspaceManager>,
    issue_ref: String,
    session_id: Option<String>,
) -> Result<Value, String> {
    let workspace_profile = workspace.current();
    tauri::async_runtime::spawn_blocking(move || {
        build_codex_transcript(&issue_ref, session_id.as_deref(), &workspace_profile)
    })
    .await
    .map_err(|error| format!("transcript read task failed: {error}"))?
}

fn build_read_surface(
    name: &str,
    allow_project_fallback: bool,
    workspace: &WorkspaceProfile,
) -> Result<Value, String> {
    let args = read_surface_args(name, workspace)
        .ok_or_else(|| format!("unknown read surface: {name}"))?;
    let result = if project_backed_surface(name) {
        match project_read_cooldown() {
            Some(cooldown) => {
                return Ok(surface_payload(
                    name,
                    skipped_project_read_command(&args, &cooldown),
                    project_read_paused_payload(&cooldown),
                    String::new(),
                    workspace,
                ));
            }
            None => run_project_read_surface(&args, workspace),
        }
    } else {
        run_shea_read_for_workspace(&args, workspace)
    };
    let parsed = if name == "status" {
        runtime_status_summary(&result, allow_project_fallback, workspace)
    } else {
        parse_json_output(&result.stdout)
    };
    Ok(surface_payload(
        name,
        command_summary_value(&result.summary),
        parsed,
        if name == "sessions" {
            result.stdout.trim().to_string()
        } else {
            String::new()
        },
        workspace,
    ))
}

fn build_codex_transcript(
    issue_ref: &str,
    session_id: Option<&str>,
    workspace: &WorkspaceProfile,
) -> Result<Value, String> {
    let local_status = run_shea_read_for_workspace(
        &read_surface_args("status", workspace).unwrap_or_default(),
        workspace,
    );
    let snapshot = parse_json_output(&local_status.stdout);
    let normalized_issue = normalize_issue_ref(issue_ref);
    let candidates = transcript_candidates(&snapshot, normalized_issue.as_deref(), session_id);

    for candidate in &candidates {
        let Some(path) = candidate.get("path").and_then(Value::as_str) else {
            continue;
        };
        let path = PathBuf::from(path);
        if !path.exists() || !path.is_file() {
            continue;
        }
        let metadata = fs::metadata(&path).ok();
        let modified_at_ms = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64);
        let content = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read local Codex transcript: {error}"))?;
        let summary = codex_conversation_summary(&content, &path);
        return Ok(json!({
            "status": "available",
            "localOnly": true,
            "path": path.display().to_string(),
            "content": "",
            "candidates": candidates,
            "threadId": summary.get("threadId").cloned().unwrap_or(Value::Null),
            "turnId": summary.get("turnId").cloned().unwrap_or(Value::Null),
            "deepLink": summary.get("deepLink").cloned().unwrap_or(Value::Null),
            "lastUserMessageAt": summary.get("lastUserMessageAt").cloned().unwrap_or(Value::Null),
            "lastAssistantMessageAt": summary.get("lastAssistantMessageAt").cloned().unwrap_or(Value::Null),
            "messageCounts": summary.get("messageCounts").cloned().unwrap_or_else(|| json!({})),
            "metadata": {
                "bytes": metadata.map(|metadata| metadata.len()).unwrap_or(0),
                "modifiedAtMs": modified_at_ms,
            },
        }));
    }

    Ok(json!({
        "status": "unavailable",
        "localOnly": true,
        "reason": if candidates.is_empty() {
            "No local Codex transcript candidate was found from session registry, runtime metadata, or .codex/sessions fallback search."
        } else {
            "Local Codex transcript candidates were found, but no readable JSONL file exists at those paths."
        },
        "path": Value::Null,
        "content": "",
        "threadId": Value::Null,
        "turnId": Value::Null,
        "deepLink": Value::Null,
        "lastUserMessageAt": Value::Null,
        "lastAssistantMessageAt": Value::Null,
        "messageCounts": {
            "user": 0,
            "assistant": 0,
        },
        "candidates": candidates,
    }))
}

fn codex_conversation_summary(content: &str, path: &Path) -> Value {
    let mut thread_id: Option<String> = None;
    let mut turn_id: Option<String> = None;
    let mut last_user_message_at = Value::Null;
    let mut last_assistant_message_at = Value::Null;
    let mut user_messages = 0_u64;
    let mut assistant_messages = 0_u64;

    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if let Some(protocol) = protocol_message_from_record(&record) {
            collect_protocol_summary(
                &protocol,
                &mut thread_id,
                &mut turn_id,
                &mut last_user_message_at,
                &mut last_assistant_message_at,
                &mut user_messages,
                &mut assistant_messages,
            );
            continue;
        }

        collect_rollout_summary(
            &record,
            &mut thread_id,
            &mut turn_id,
            &mut last_user_message_at,
            &mut last_assistant_message_at,
            &mut user_messages,
            &mut assistant_messages,
        );
    }

    if thread_id.is_none() {
        thread_id = uuid_like_parts(&path.display().to_string())
            .into_iter()
            .next();
    }

    json!({
        "threadId": thread_id.clone(),
        "turnId": turn_id,
        "deepLink": thread_id.map(|id| format!("codex://threads/{id}")),
        "lastUserMessageAt": last_user_message_at,
        "lastAssistantMessageAt": last_assistant_message_at,
        "messageCounts": {
            "user": user_messages,
            "assistant": assistant_messages,
        },
    })
}

fn protocol_message_from_record(record: &Value) -> Option<Value> {
    record.get("direction").and_then(Value::as_str)?;
    let line = record.get("line").and_then(Value::as_str)?;
    serde_json::from_str::<Value>(line).ok()
}

fn collect_protocol_summary(
    message: &Value,
    thread_id: &mut Option<String>,
    turn_id: &mut Option<String>,
    last_user_message_at: &mut Value,
    last_assistant_message_at: &mut Value,
    user_messages: &mut u64,
    assistant_messages: &mut u64,
) {
    capture_string(thread_id, message.get("threadId"));
    capture_string(turn_id, message.get("turnId"));

    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = message.get("params").unwrap_or(&Value::Null);

    if method == "thread/start" {
        if let Some(thread) = message
            .get("result")
            .and_then(|result| result.get("thread"))
        {
            capture_string(thread_id, thread.get("id"));
            capture_string(thread_id, thread.get("sessionId"));
        }
    }

    if method == "turn/started" {
        capture_string(thread_id, params.get("threadId"));
        if let Some(turn) = params.get("turn") {
            capture_string(turn_id, turn.get("id"));
            if let Some(timestamp) = timestamp_value(
                turn.get("startedAt")
                    .or_else(|| turn.get("startedAtMs"))
                    .or_else(|| params.get("startedAtMs")),
            ) {
                *last_user_message_at = timestamp;
            }
        }
    } else if method == "turn/start" {
        if let Some(timestamp) = timestamp_value(message.get("completedAtMs")) {
            *last_user_message_at = timestamp;
        }
        *user_messages = user_messages.saturating_add(1);
    }

    if method == "item/completed" {
        let item = params.get("item").unwrap_or(&Value::Null);
        if item.get("type").and_then(Value::as_str) == Some("agentMessage") {
            if let Some(timestamp) = timestamp_value(
                params
                    .get("completedAtMs")
                    .or_else(|| item.get("completedAtMs"))
                    .or_else(|| message.get("completedAtMs")),
            ) {
                *last_assistant_message_at = timestamp;
            }
            *assistant_messages = assistant_messages.saturating_add(1);
        }
    }
}

fn collect_rollout_summary(
    record: &Value,
    thread_id: &mut Option<String>,
    turn_id: &mut Option<String>,
    last_user_message_at: &mut Value,
    last_assistant_message_at: &mut Value,
    user_messages: &mut u64,
    assistant_messages: &mut u64,
) {
    let wrapper_type = record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload = record.get("payload").unwrap_or(&Value::Null);
    let item = record
        .get("item")
        .or_else(|| payload.get("item"))
        .unwrap_or(payload);
    let item_type = item
        .get("type")
        .or_else(|| record.get("event"))
        .or_else(|| record.get("kind"))
        .or_else(|| record.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let role = item.get("role").and_then(Value::as_str).unwrap_or_default();
    let timestamp = timestamp_value(record.get("timestamp"));

    if wrapper_type == "session_meta" {
        capture_string(thread_id, payload.get("id"));
        capture_string(thread_id, item.get("id"));
        return;
    }
    capture_string(
        thread_id,
        record.get("threadId").or_else(|| payload.get("threadId")),
    );
    capture_string(
        turn_id,
        record.get("turnId").or_else(|| payload.get("turnId")),
    );

    if item_type == "user_message" || role == "user" {
        if let Some(timestamp) = timestamp {
            *last_user_message_at = timestamp;
        }
        *user_messages = user_messages.saturating_add(1);
        return;
    }
    if item_type == "agent_message"
        || item_type == "assistant_message"
        || role == "assistant"
        || item_type == "final_answer"
    {
        if let Some(timestamp) = timestamp {
            *last_assistant_message_at = timestamp;
        }
        *assistant_messages = assistant_messages.saturating_add(1);
    }
}

fn capture_string(target: &mut Option<String>, value: Option<&Value>) {
    if target.is_some() {
        return;
    }
    if let Some(value) = value.and_then(Value::as_str).map(str::trim) {
        if !value.is_empty() {
            *target = Some(value.to_string());
        }
    }
}

fn timestamp_value(value: Option<&Value>) -> Option<Value> {
    match value? {
        Value::Number(number) => {
            let raw = number
                .as_u64()
                .or_else(|| number.as_i64().and_then(|value| u64::try_from(value).ok()))?;
            if raw == 0 {
                return None;
            }
            Some(json!(if raw < 100_000_000_000 {
                raw * 1000
            } else {
                raw
            }))
        }
        Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                Some(json!(value))
            }
        }
        _ => None,
    }
}

fn transcript_candidates(
    snapshot: &Value,
    issue_ref: Option<&str>,
    session_id: Option<&str>,
) -> Vec<Value> {
    let mut candidates = Vec::new();
    push_registry_transcript_candidates(&mut candidates, snapshot, issue_ref, session_id);
    push_app_server_protocol_candidates(&mut candidates, snapshot, issue_ref);
    if let Some(session_id) = session_id {
        push_session_store_candidates(&mut candidates, session_id, "runtime_session_id");
    }
    if let Some(issue_ref) = issue_ref {
        for session in snapshot
            .get("sessions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if normalize_issue_ref(
                session
                    .get("issue_identifier")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .as_deref()
                != Some(issue_ref)
            {
                continue;
            }
            for key in ["session_id", "session_name", "run_id", "instance_name"] {
                if let Some(value) = session.get(key).and_then(Value::as_str) {
                    push_session_store_candidates(&mut candidates, value, key);
                }
            }
        }
    }
    dedupe_candidates(candidates)
}

fn push_registry_transcript_candidates(
    candidates: &mut Vec<Value>,
    snapshot: &Value,
    issue_ref: Option<&str>,
    session_id: Option<&str>,
) {
    for session in snapshot
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let session_issue = normalize_issue_ref(
            session
                .get("issue_identifier")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        let issue_matches = issue_ref.is_some_and(|issue| session_issue.as_deref() == Some(issue));
        let session_matches = session_id.is_some_and(|id| {
            ["session_id", "session_name", "run_id", "instance_name"]
                .iter()
                .any(|key| {
                    session
                        .get(key)
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.contains(id) || id.contains(value))
                })
        });
        if !issue_matches && !session_matches {
            continue;
        }
        for key in [
            "transcript_path",
            "codex_transcript_path",
            "jsonl_path",
            "protocol_artifact_path",
            "log_path",
        ] {
            if let Some(path) = session
                .get(key)
                .and_then(Value::as_str)
                .filter(|path| path.ends_with(".jsonl"))
            {
                candidates.push(json!({
                    "source": format!("session_registry.{key}"),
                    "path": path,
                    "session": session.get("session_name").or_else(|| session.get("run_id")).cloned().unwrap_or(Value::Null),
                    "issue": session_issue,
                }));
            }
        }
        if let Some(path) = session
            .get("log_path")
            .and_then(Value::as_str)
            .filter(|path| path.ends_with(".events.json"))
        {
            let protocol_path =
                path.trim_end_matches(".events.json").to_string() + ".protocol.jsonl";
            candidates.push(json!({
                "source": "session_registry.log_path_protocol_sibling",
                "path": protocol_path,
                "session": session.get("session_name").or_else(|| session.get("run_id")).cloned().unwrap_or(Value::Null),
                "issue": session_issue,
            }));
        }
    }
}

fn push_app_server_protocol_candidates(
    candidates: &mut Vec<Value>,
    snapshot: &Value,
    issue_ref: Option<&str>,
) {
    let Some(issue_ref) = issue_ref else {
        return;
    };
    let Some(issue_number) = normalize_issue_ref(issue_ref)
        .and_then(|issue| issue.strip_prefix('#').map(str::to_string))
    else {
        return;
    };
    let Some(event_log_path) = snapshot.get("event_log_path").and_then(Value::as_str) else {
        return;
    };
    let Some(logs_root) = Path::new(event_log_path).parent() else {
        return;
    };
    let app_server_root = logs_root.join("app-server");
    let prefix = format!("{issue_number}-");
    for path in find_app_server_protocol_jsonl(&app_server_root, &prefix, 20) {
        candidates.push(json!({
            "source": "app_server_protocol_issue_fallback",
            "path": path.display().to_string(),
            "issue": format!("#{issue_number}"),
        }));
    }
}

fn push_session_store_candidates(candidates: &mut Vec<Value>, needle: &str, source: &str) {
    let Some(root) = codex_sessions_root() else {
        return;
    };
    for needle in session_store_needles(needle) {
        for path in find_rollout_jsonl(&root, &needle, 40) {
            candidates.push(json!({
                "source": source,
                "path": path.display().to_string(),
                "session": needle,
            }));
        }
    }
}

fn session_store_needles(needle: &str) -> Vec<String> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut needles = vec![needle.to_string()];
    for part in uuid_like_parts(needle) {
        if !needles.iter().any(|existing| existing == &part) {
            needles.push(part);
        }
    }
    needles
}

fn uuid_like_parts(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for index in value.char_indices().map(|(index, _)| index) {
        let Some(candidate) = value.get(index..index.saturating_add(36)) else {
            continue;
        };
        if candidate.len() != 36 {
            continue;
        }
        let bytes = candidate.as_bytes();
        let valid = [8, 13, 18, 23]
            .iter()
            .all(|position| bytes[*position] == b'-')
            && bytes.iter().enumerate().all(|(position, byte)| {
                [8, 13, 18, 23].contains(&position) || byte.is_ascii_hexdigit()
            });
        if valid {
            parts.push(candidate.to_string());
        }
    }
    parts
}

fn find_app_server_protocol_jsonl(root: &Path, issue_prefix: &str, limit: usize) -> Vec<PathBuf> {
    let mut matches = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return matches;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(issue_prefix) && name.ends_with(".protocol.jsonl"))
        {
            matches.push(path);
        }
    }
    matches.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| std::cmp::Reverse(duration.as_millis()))
    });
    matches.truncate(limit);
    matches
}

fn find_rollout_jsonl(root: &Path, needle: &str, limit: usize) -> Vec<PathBuf> {
    let mut matches = Vec::new();
    collect_rollout_jsonl(root, needle, limit, &mut matches);
    matches.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| std::cmp::Reverse(duration.as_millis()))
    });
    matches.truncate(limit);
    matches
}

fn collect_rollout_jsonl(root: &Path, needle: &str, limit: usize, matches: &mut Vec<PathBuf>) {
    if matches.len() >= limit {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rollout_jsonl(&path, needle, limit, matches);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with("rollout-") && name.ends_with(".jsonl") && name.contains(needle)
            })
        {
            matches.push(path);
        }
        if matches.len() >= limit {
            return;
        }
    }
}

fn codex_sessions_root() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .map(|path| path.join("sessions"))
}

fn dedupe_candidates(candidates: Vec<Value>) -> Vec<Value> {
    let mut seen = BTreeMap::new();
    for candidate in candidates {
        let key = candidate
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if key.is_empty() || seen.contains_key(&key) {
            continue;
        }
        seen.insert(key, candidate);
    }
    seen.into_values().collect()
}

fn normalize_issue_ref(value: &str) -> Option<String> {
    let digits: String = value
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        Some(format!("#{digits}"))
    }
}

fn build_operator_overview(scope: &str, workspace: &WorkspaceProfile) -> Result<Value, String> {
    let generated_at = timestamp_iso_like();
    let github_queue_args = read_surface_args("githubQueue", workspace).unwrap_or_default();
    let workflow_path = workspace.workflow_path.as_str();
    let target_context = TargetContext::from_workspace(workspace).to_value();

    if scope == "fast" {
        let commands = json!({
            "autopilot": pending_result(&["autopilot", "plan", workflow_path, "--json"], "Deferred to full overview."),
            "doctor": pending_result(&["doctor", workflow_path, "--json"], "Deferred to full overview."),
            "review": pending_result(&["review", "status", workflow_path, "--json"], "Deferred to full overview."),
            "sessions": pending_result(&["session", "list", workflow_path], "Deferred to background read."),
            "status": pending_result(&["status", "show", workflow_path, "--json"], "Deferred to background read."),
            "githubQueue": pending_result(&["project", "state", workflow_path, "--json"], "Deferred to background Project queue read."),
        });
        return Ok(json!({
            "generatedAt": generated_at,
            "workflowPath": workflow_path,
            "workspace": workspace,
            "targetContext": target_context,
            "scope": "fast",
            "commands": commands,
            "autopilot": Value::Null,
            "doctor": Value::Null,
            "review": Value::Null,
            "sessionsText": "",
            "localStatus": Value::Null,
            "githubQueue": Value::Null,
            "healthy": true,
        }));
    }

    let runtime = run_shea_read_for_workspace(
        &read_surface_args("status", workspace).unwrap_or_default(),
        workspace,
    );
    let autopilot = run_project_read_surface_or_skip(
        &read_surface_args("autopilot", workspace).unwrap_or_default(),
        workspace,
    );
    let doctor = run_project_read_surface_or_skip(
        &read_surface_args("doctor", workspace).unwrap_or_default(),
        workspace,
    );
    let review = run_project_read_surface_or_skip(
        &read_surface_args("review", workspace).unwrap_or_default(),
        workspace,
    );
    let sessions = run_shea_read_for_workspace(
        &read_surface_args("sessions", workspace).unwrap_or_default(),
        workspace,
    );
    let github_queue = run_project_read_surface_or_skip(&github_queue_args, workspace);
    let healthy = [autopilot.summary.ok, doctor.summary.ok, review.summary.ok]
        .iter()
        .any(|ok| *ok);

    Ok(json!({
        "generatedAt": generated_at,
        "workflowPath": workflow_path,
        "workspace": workspace,
        "targetContext": target_context,
        "commands": {
            "autopilot": command_summary_value(&autopilot.summary),
            "doctor": command_summary_value(&doctor.summary),
            "review": command_summary_value(&review.summary),
            "sessions": command_summary_value(&sessions.summary),
            "status": command_summary_value(&runtime.summary),
            "githubQueue": command_summary_value(&github_queue.summary),
        },
        "autopilot": parse_json_output(&autopilot.stdout),
        "doctor": parse_json_output(&doctor.stdout),
        "review": parse_json_output(&review.stdout),
        "sessionsText": sessions.stdout.trim(),
        "localStatus": runtime_status_summary(&runtime, false, workspace),
        "githubQueue": parse_json_output(&github_queue.stdout),
        "healthy": healthy,
    }))
}

fn read_surface_args(name: &str, workspace: &WorkspaceProfile) -> Option<Vec<String>> {
    let workflow_path = workspace.workflow_path.clone();
    match name {
        "autopilot" => Some(vec![
            "autopilot".into(),
            "plan".into(),
            workflow_path,
            "--json".into(),
        ]),
        "doctor" => Some(vec!["doctor".into(), workflow_path, "--json".into()]),
        "review" => Some(vec![
            "review".into(),
            "status".into(),
            workflow_path,
            "--json".into(),
        ]),
        "sessions" => Some(vec!["session".into(), "list".into(), workflow_path]),
        "status" => Some(vec![
            "status".into(),
            "show".into(),
            workflow_path,
            "--json".into(),
        ]),
        "githubQueue" => Some(vec![
            "project".into(),
            "state".into(),
            workflow_path,
            "--json".into(),
        ]),
        _ => None,
    }
}

fn runtime_status_summary(
    result: &CommandRun,
    allow_project_fallback: bool,
    workspace: &WorkspaceProfile,
) -> Value {
    let snapshot = parse_json_output(&result.stdout);
    if snapshot.is_null() {
        return Value::Null;
    }
    let running_count = snapshot
        .get("running")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let planned_count = snapshot
        .get("planned")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let retrying_count = snapshot
        .get("retrying")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let session_count = snapshot
        .get("sessions")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let integration_gap_count = snapshot
        .get("integration_gaps")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let issue_worktrees = git_worktree_issue_inventory(workspace);
    let project_issues = if allow_project_fallback && project_read_cooldown().is_none() {
        project_issue_readbacks(&issue_worktrees, &snapshot, workspace)
    } else {
        Value::Array(vec![])
    };
    let completed_issue_worktrees =
        completed_issue_worktrees(&snapshot, &issue_worktrees, &project_issues);
    json!({
        "source": "shea-symphony status show --json",
        "targetContext": TargetContext::from_workspace(workspace).to_value(),
        "runningCount": running_count,
        "plannedCount": planned_count,
        "retryingCount": retrying_count,
        "sessionCount": session_count,
        "integrationGapCount": integration_gap_count,
        "issueWorktrees": issue_worktrees,
        "projectIssues": project_issues,
        "projectFallbackAllowed": allow_project_fallback,
        "projectFallbackPaused": project_read_cooldown().is_some(),
        "completedIssueWorktrees": completed_issue_worktrees,
        "eventLogPath": snapshot.get("event_log_path").cloned().unwrap_or(Value::Null),
        "snapshot": snapshot,
    })
}

fn completed_issue_worktrees(
    snapshot: &Value,
    issue_worktrees: &Value,
    project_issues: &Value,
) -> Value {
    let mut worktrees_by_issue = BTreeMap::new();
    for worktree in issue_worktrees.as_array().into_iter().flatten() {
        if let Some(issue) = worktree.get("issue").and_then(Value::as_str) {
            worktrees_by_issue.insert(issue.to_string(), worktree.clone());
        }
    }

    let mut completed = BTreeMap::new();
    if let Some(sessions) = snapshot.get("sessions").and_then(Value::as_array) {
        for session in sessions {
            let status = session
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !matches!(status, "completed" | "recorded") {
                continue;
            }
            let Some(issue) = session
                .get("issue_identifier")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            let Some(worktree) = worktrees_by_issue.get(&issue) else {
                continue;
            };
            let session_source = session
                .get("session_source")
                .and_then(Value::as_str)
                .unwrap_or("session_registry");
            let is_project_readback_cache = session_source == "project_readback_cache";
            let updated_at = if is_project_readback_cache {
                Value::Null
            } else {
                session.get("updated_at_ms").cloned().unwrap_or(Value::Null)
            };
            let title = session.get("issue_title").cloned().unwrap_or(Value::Null);
            let state = if is_project_readback_cache {
                session.get("project_state").cloned().unwrap_or(Value::Null)
            } else {
                Value::String("Done".into())
            };
            let url = session.get("project_url").cloned().unwrap_or(Value::Null);
            let last_progress_source = if updated_at.is_null() {
                Value::String("unavailable".into())
            } else {
                Value::String("session_registry.updated_at_ms".into())
            };
            completed.insert(
                issue.clone(),
                json!({
                    "issue": issue,
                    "title": title,
                    "state": state,
                    "lane": session.get("lane").cloned().unwrap_or(Value::Null),
                    "url": url,
                    "completedAt": updated_at,
                    "createdAt": session.get("started_at_ms").cloned().unwrap_or_else(|| worktree.get("createdAt").cloned().unwrap_or(Value::Null)),
                    "lastProgressAt": updated_at,
                    "lastProgressSource": last_progress_source,
                    "path": worktree.get("path").cloned().unwrap_or(Value::Null),
                    "branch": worktree.get("branch").cloned().unwrap_or(Value::Null),
                    "head": worktree.get("head").cloned().unwrap_or(Value::Null),
                    "lastModified": worktree.get("lastModified").cloned().unwrap_or(Value::Null),
                    "lastModifiedSource": "git_worktree_filesystem",
                    "treeState": worktree.get("treeState").cloned().unwrap_or(Value::Null),
                    "diskBytes": worktree.get("diskBytes").cloned().unwrap_or(Value::Null),
                    "evidence": session.get("evidence").cloned().unwrap_or(Value::Null),
                    "artifactSource": if is_project_readback_cache { "project_readback_cache" } else { "session_registry" },
                    "timestampSources": {
                        "lastProgress": {
                            "source": if is_project_readback_cache { "unavailable" } else { "session_registry.updated_at_ms" },
                            "meaning": if is_project_readback_cache { "No durable handoff progress evidence is available from the local read surface." } else { "Session registry lane progress timestamp." }
                        },
                        "lastModified": {
                            "source": "git_worktree_filesystem",
                            "meaning": "Latest counted local worktree file modification time."
                        }
                    },
                }),
            );
        }
    }

    for project_issue in project_issues.as_array().into_iter().flatten() {
        let Some(issue) = project_issue.get("identifier").and_then(Value::as_str) else {
            continue;
        };
        let Some(worktree) = worktrees_by_issue.get(issue) else {
            continue;
        };
        let previous = completed.get(issue);
        let last_progress_at = previous
            .and_then(|entry| entry.get("lastProgressAt"))
            .cloned()
            .unwrap_or(Value::Null);
        let last_progress_source = previous
            .and_then(|entry| entry.get("lastProgressSource"))
            .cloned()
            .unwrap_or_else(|| Value::String("unavailable".into()));
        let project_updated_at = project_issue
            .get("updatedAt")
            .or_else(|| project_issue.get("updated_at"))
            .cloned()
            .unwrap_or(Value::Null);
        completed.insert(
            issue.to_string(),
            json!({
                "issue": issue,
                "title": project_issue.get("title").cloned().unwrap_or(Value::Null),
                "state": project_issue.get("state").cloned().unwrap_or(Value::Null),
                "lane": previous.and_then(|entry| entry.get("lane")).cloned().unwrap_or(Value::Null),
                "url": project_issue.get("url").cloned().unwrap_or(Value::Null),
                "completedAt": previous.and_then(|entry| entry.get("completedAt")).cloned().unwrap_or(Value::Null),
                "createdAt": previous.and_then(|entry| entry.get("createdAt")).cloned().unwrap_or_else(|| worktree.get("createdAt").cloned().unwrap_or(Value::Null)),
                "lastProgressAt": last_progress_at,
                "lastProgressSource": last_progress_source,
                "projectUpdatedAt": project_updated_at,
                "projectUpdatedAtSource": "github_project_issue.updatedAt",
                "path": worktree.get("path").cloned().unwrap_or(Value::Null),
                "branch": worktree.get("branch").cloned().unwrap_or(Value::Null),
                "head": worktree.get("head").cloned().unwrap_or(Value::Null),
                "lastModified": worktree.get("lastModified").cloned().unwrap_or(Value::Null),
                "lastModifiedSource": "git_worktree_filesystem",
                "treeState": worktree.get("treeState").cloned().unwrap_or(Value::Null),
                "diskBytes": worktree.get("diskBytes").cloned().unwrap_or(Value::Null),
                "evidence": previous.and_then(|entry| entry.get("evidence")).cloned().unwrap_or_else(|| Value::String("project issue readback supplies title/state/url; no handoff progress evidence found".into())),
                "artifactSource": previous.and_then(|entry| entry.get("artifactSource")).cloned().unwrap_or_else(|| Value::String("project_issue_readback".into())),
                "timestampSources": {
                    "lastProgress": {
                        "source": previous
                            .and_then(|entry| entry.pointer("/timestampSources/lastProgress/source"))
                            .cloned()
                            .unwrap_or_else(|| Value::String("unavailable".into())),
                        "meaning": previous
                            .and_then(|entry| entry.pointer("/timestampSources/lastProgress/meaning"))
                            .cloned()
                            .unwrap_or_else(|| Value::String("No durable handoff progress evidence is available from the local read surface.".into()))
                    },
                    "lastModified": {
                        "source": "git_worktree_filesystem",
                        "meaning": "Latest counted local worktree file modification time."
                    },
                    "projectUpdatedAt": {
                        "source": "github_project_issue.updatedAt",
                        "meaning": "Broad Project/GitHub issue readback timestamp retained as metadata only; it must not drive Last Progress."
                    }
                },
            }),
        );
    }
    Value::Array(completed.into_values().collect())
}

fn project_issue_readbacks(
    issue_worktrees: &Value,
    snapshot: &Value,
    workspace: &WorkspaceProfile,
) -> Value {
    let registry_issues = session_registry_issue_refs(snapshot);
    let mut issues = BTreeMap::new();
    for worktree in issue_worktrees.as_array().into_iter().flatten() {
        let Some(issue) = worktree.get("issue").and_then(Value::as_str) else {
            continue;
        };
        if registry_issues.contains_key(issue) || issues.contains_key(issue) {
            continue;
        }
        if let Some(readback) = project_issue_readback(issue, workspace) {
            let _ = write_project_readback_session(snapshot, worktree, &readback);
            issues.insert(issue.to_string(), readback);
        }
    }
    Value::Array(issues.into_values().collect())
}

fn session_registry_issue_refs(snapshot: &Value) -> BTreeMap<String, Value> {
    let mut issues = BTreeMap::new();
    for session in snapshot
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let status = session
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(status, "completed" | "recorded") {
            continue;
        }
        let Some(issue) = session.get("issue_identifier").and_then(Value::as_str) else {
            continue;
        };
        let has_title = session
            .get("issue_title")
            .and_then(Value::as_str)
            .is_some_and(|title| !title.trim().is_empty());
        let has_project_state = session
            .get("project_state")
            .and_then(Value::as_str)
            .is_some();
        if has_title && (status == "completed" || has_project_state) {
            issues.insert(issue.to_string(), session.clone());
        }
    }
    issues
}

fn project_issue_readback(issue: &str, workspace: &WorkspaceProfile) -> Option<Value> {
    if project_read_cooldown().is_some() {
        return None;
    }
    let output = shea_command_for_workspace(
        &[
            "project",
            "issue",
            workspace.workflow_path.as_str(),
            issue,
            "--json",
        ],
        workspace,
    )
    .ok()?
    .output()
    .ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if project_rate_limit_detected(&stderr) {
        record_project_rate_limit_cooldown(&stderr);
    }
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

fn write_project_readback_session(
    snapshot: &Value,
    worktree: &Value,
    issue: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(registry_path) = session_registry_path_from_snapshot(snapshot) else {
        return Ok(());
    };
    let Some(identifier) = issue.get("identifier").and_then(Value::as_str) else {
        return Ok(());
    };
    let number = identifier.trim_start_matches('#');
    let session_name = format!("project-readback-issue-{number}");
    let now_ms = unix_timestamp_ms();
    let mut registry = fs::read_to_string(&registry_path)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .unwrap_or_else(|| json!({ "sessions": [] }));
    let sessions = registry
        .get_mut("sessions")
        .and_then(Value::as_array_mut)
        .ok_or("session registry payload missing sessions array")?;
    let record = json!({
        "issue_id": issue.get("id").cloned().unwrap_or(Value::Null),
        "issue_identifier": identifier,
        "issue_title": issue.get("title").cloned().unwrap_or(Value::Null),
        "project_state": issue.get("state").cloned().unwrap_or(Value::Null),
        "project_url": issue.get("url").cloned().unwrap_or(Value::Null),
        "lane": "project",
        "run_id": "project-readback-cache",
        "session_source": "project_readback_cache",
        "actor_role": "operator-ui",
        "actor_label": "Lane Views",
        "git_author": Value::Null,
        "profile_id": Value::Null,
        "instance_name": Value::Null,
        "worktree": worktree.get("path").cloned().unwrap_or(Value::String(String::new())),
        "branch": worktree.get("branch").cloned().unwrap_or(Value::Null),
        "backend": "project-readback",
        "session_name": session_name,
        "pane_target": "",
        "prompt_artifact_path": "",
        "log_path": "",
        "attach_command": "",
        "attempt": 0,
        "status": "recorded",
        "started_at_ms": now_ms,
        "updated_at_ms": now_ms,
    });
    if let Some(existing) = sessions.iter_mut().find(|session| {
        session.get("session_name").and_then(Value::as_str) == Some(session_name.as_str())
    }) {
        *existing = record;
    } else {
        sessions.push(record);
    }
    if let Some(parent) = registry_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(registry_path, serde_json::to_string_pretty(&registry)?)?;
    Ok(())
}

fn session_registry_path_from_snapshot(snapshot: &Value) -> Option<PathBuf> {
    let event_log_path = snapshot.get("event_log_path").and_then(Value::as_str)?;
    let default_root = Path::new(event_log_path).parent()?.parent()?;
    Some(default_root.join("sessions").join("session-registry.json"))
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn git_worktree_issue_inventory(workspace: &WorkspaceProfile) -> Value {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace.target_path())
        .args(["worktree", "list", "--porcelain"])
        .output();
    let Ok(output) = output else {
        return Value::Array(vec![]);
    };
    if !output.status.success() {
        return Value::Array(vec![]);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Value::Array(
        parse_git_worktree_porcelain(&text)
            .into_iter()
            .filter_map(|worktree| {
                let issue = infer_issue_ref(worktree.branch.as_deref(), Path::new(&worktree.path))?;
                let stats = worktree_stats(&worktree.path);
                Some(json!({
                    "issue": issue,
                    "path": worktree.path,
                    "branch": worktree.branch,
                    "head": worktree.head,
                    "createdAt": worktree_created_at(&worktree.path),
                    "lastModified": stats.last_modified_ms.map(Value::from).unwrap_or(Value::Null),
                    "treeState": worktree_tree_state(&worktree.path),
                    "diskBytes": Value::from(stats.disk_bytes),
                    "evidence": "git worktree list --porcelain",
                }))
            })
            .collect(),
    )
}

#[derive(Default)]
struct LocalWorktree {
    path: String,
    head: Option<String>,
    branch: Option<String>,
}

fn parse_git_worktree_porcelain(input: &str) -> Vec<LocalWorktree> {
    let mut worktrees = Vec::new();
    let mut current: Option<LocalWorktree> = None;

    for line in input.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(worktree) = current.take() {
                worktrees.push(worktree);
            }
            current = Some(LocalWorktree {
                path: path.to_string(),
                ..LocalWorktree::default()
            });
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            if let Some(worktree) = &mut current {
                worktree.head = Some(head.to_string());
            }
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if let Some(worktree) = &mut current {
                worktree.branch = Some(branch.trim_start_matches("refs/heads/").to_string());
            }
        }
    }

    if let Some(worktree) = current {
        worktrees.push(worktree);
    }
    worktrees
}

fn infer_issue_ref(branch: Option<&str>, path: &Path) -> Option<String> {
    branch
        .and_then(issue_ref_from_text)
        .or_else(|| {
            path.file_name()
                .and_then(|name| issue_ref_from_text(&name.to_string_lossy()))
        })
        .or_else(|| issue_ref_from_text(&path.display().to_string()))
}

fn issue_ref_from_text(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    for marker in ["issue-", "issue_", "issue/", "#"] {
        if let Some(index) = lower.find(marker) {
            let suffix = &lower[index + marker.len()..];
            let digits: String = suffix
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if !digits.is_empty() {
                return Some(format!("#{digits}"));
            }
        }
    }
    None
}

#[derive(Default)]
struct WorktreeStats {
    disk_bytes: u64,
    last_modified_ms: Option<u64>,
}

fn worktree_created_at(path: &str) -> Value {
    let created = fs::metadata(path)
        .and_then(|metadata| metadata.created())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64);
    created.map(Value::from).unwrap_or(Value::Null)
}

fn worktree_tree_state(path: &str) -> Value {
    let output = Command::new("git")
        .args(["-C", path, "status", "--porcelain"])
        .output();
    let Ok(output) = output else {
        return Value::String("unknown".into());
    };
    if !output.status.success() {
        return Value::String("unknown".into());
    }
    if output.stdout.is_empty() {
        Value::String("clean".into())
    } else {
        Value::String("dirty".into())
    }
}

fn worktree_stats(path: &str) -> WorktreeStats {
    let mut stats = WorktreeStats::default();
    accumulate_worktree_stats(Path::new(path), &mut stats);
    stats
}

fn accumulate_worktree_stats(path: &Path, stats: &mut WorktreeStats) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_file() {
        stats.disk_bytes = stats.disk_bytes.saturating_add(metadata.len());
    }
    if should_count_modified_time(path) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                let ms = duration.as_millis() as u64;
                stats.last_modified_ms =
                    Some(stats.last_modified_ms.map_or(ms, |current| current.max(ms)));
            }
        }
    }
    if !metadata.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        accumulate_worktree_stats(&entry.path(), stats);
    }
}

fn should_count_modified_time(path: &Path) -> bool {
    !path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(
            name.as_ref(),
            ".git" | "target" | "node_modules" | ".svelte-kit" | "dist" | "build"
        )
    })
}

fn surface_payload(
    name: &str,
    command: Value,
    parsed: Value,
    text: String,
    workspace: &WorkspaceProfile,
) -> Value {
    json!({
        "name": name,
        "generatedAt": timestamp_iso_like(),
        "workflowPath": workspace.workflow_path.as_str(),
        "workspace": workspace,
        "targetContext": TargetContext::from_workspace(workspace).to_value(),
        "command": command,
        "parsed": parsed,
        "text": text,
    })
}

#[derive(Debug, Clone)]
struct ProjectReadCooldown {
    until_ms: u128,
    reason: String,
}

fn project_cooldown_cell() -> &'static Mutex<Option<ProjectReadCooldown>> {
    static CELL: OnceLock<Mutex<Option<ProjectReadCooldown>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

fn project_backed_surface(name: &str) -> bool {
    PROJECT_READ_SURFACES.contains(&name)
}

fn run_project_read_surface_or_skip(args: &[String], workspace: &WorkspaceProfile) -> CommandRun {
    if let Some(cooldown) = project_read_cooldown() {
        return skipped_command_run(args, skipped_project_read_stderr(&cooldown));
    }
    run_project_read_surface(args, workspace)
}

fn run_project_read_surface(args: &[String], workspace: &WorkspaceProfile) -> CommandRun {
    let result = run_shea_read_for_workspace(args, workspace);
    if !result.summary.ok
        && (project_rate_limit_detected(&result.summary.stderr)
            || project_rate_limit_detected(&result.summary.stdout_preview)
            || project_rate_limit_detected(&result.stdout))
    {
        record_project_rate_limit_cooldown(&result.summary.stderr);
    }
    result
}

fn project_read_cooldown() -> Option<ProjectReadCooldown> {
    let now = timestamp_iso_like();
    let mut guard = project_cooldown_cell().lock().ok()?;
    if guard
        .as_ref()
        .is_some_and(|cooldown| cooldown.until_ms > now)
    {
        return guard.clone();
    }
    *guard = None;
    None
}

fn record_project_rate_limit_cooldown(stderr: &str) -> ProjectReadCooldown {
    let now = timestamp_iso_like();
    let reset_ms = github_graphql_reset_ms()
        .filter(|reset| *reset > now)
        .unwrap_or(now + DEFAULT_PROJECT_RATE_LIMIT_COOLDOWN_MS);
    let cooldown = ProjectReadCooldown {
        until_ms: reset_ms,
        reason: project_rate_limit_reason(stderr),
    };
    if let Ok(mut guard) = project_cooldown_cell().lock() {
        *guard = Some(cooldown.clone());
    }
    cooldown
}

fn project_rate_limit_detected(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("failure_kind=rate_limit")
        || lower.contains("kind=rate_limit")
        || lower.contains("api rate limit already exceeded")
        || lower.contains("api rate limit exceeded")
        || lower.contains("graphql resource limit exceeded")
}

fn project_rate_limit_reason(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        "GitHub Project GraphQL rate limit was reached.".into()
    } else {
        trimmed
            .lines()
            .next()
            .unwrap_or(trimmed)
            .chars()
            .take(240)
            .collect()
    }
}

fn github_graphql_reset_ms() -> Option<u128> {
    let output = Command::new("gh")
        .args(["api", "rate_limit"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: Value = serde_json::from_slice(&output.stdout).ok()?;
    value
        .pointer("/resources/graphql/reset")
        .and_then(Value::as_u64)
        .map(|seconds| u128::from(seconds) * 1000)
}

fn skipped_command_run(args: &[String], stderr: String) -> CommandRun {
    CommandRun {
        summary: crate::cli::CommandSummary {
            ok: false,
            args: args.to_vec(),
            exit_code: None,
            signal: Some("project-rate-limit-cooldown".into()),
            timed_out: false,
            duration_ms: 0,
            stderr,
            stdout_preview: String::new(),
        },
        stdout: String::new(),
    }
}

fn skipped_project_read_stderr(cooldown: &ProjectReadCooldown) -> String {
    format!(
        "Project read paused until {} because GitHub Project GraphQL rate limit was reached: {}",
        cooldown.until_ms, cooldown.reason
    )
}

fn skipped_project_read_command(args: &[String], cooldown: &ProjectReadCooldown) -> Value {
    json!({
        "ok": false,
        "skipped": true,
        "projectReadPaused": true,
        "rateLimitResetAtMs": cooldown.until_ms,
        "args": args,
        "exitCode": Value::Null,
        "signal": "project-rate-limit-cooldown",
        "timedOut": false,
        "durationMs": 0,
        "stderr": skipped_project_read_stderr(cooldown),
        "stdoutPreview": "",
    })
}

fn project_read_paused_payload(cooldown: &ProjectReadCooldown) -> Value {
    json!({
        "projectStateAccess": "paused",
        "trusted": false,
        "failureKind": "rate_limit",
        "projectReadPaused": true,
        "rateLimitResetAtMs": cooldown.until_ms,
        "reason": cooldown.reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_skills_status_surface_is_not_registered() {
        let workspace = WorkspaceProfile::self_targeted(PathBuf::from("/tmp/shea-symphony"));

        assert!(read_surface_args("skills", &workspace).is_none());
    }

    #[test]
    fn detects_project_rate_limit_messages() {
        assert!(project_rate_limit_detected("failure_kind=rate_limit"));
        assert!(project_rate_limit_detected(
            "gh: API rate limit already exceeded for user ID 123"
        ));
        assert!(project_rate_limit_detected(
            "GitHub GraphQL operation failed after 2 attempts kind=rate_limit"
        ));
        assert!(!project_rate_limit_detected("project_state_access=ok"));
    }

    #[test]
    fn skipped_project_surface_reports_cooldown_payload() {
        let cooldown = ProjectReadCooldown {
            until_ms: 1_780_415_055_000,
            reason: "rate limited".into(),
        };
        let args = vec!["project".into(), "state".into()];
        let command = skipped_project_read_command(&args, &cooldown);
        let parsed = project_read_paused_payload(&cooldown);

        assert_eq!(command["projectReadPaused"], true);
        assert_eq!(
            command["rateLimitResetAtMs"].as_u64(),
            Some(1_780_415_055_000)
        );
        assert_eq!(parsed["projectStateAccess"], "paused");
        assert_eq!(parsed["failureKind"], "rate_limit");
    }

    #[test]
    fn project_updated_at_does_not_populate_completed_worktree_progress() {
        let snapshot = json!({ "sessions": [] });
        let issue_worktrees = json!([
            {
                "issue": "#244",
                "path": "/tmp/issue-244",
                "branch": "feature/issue-244",
                "head": "abc1234",
                "createdAt": 1_000,
                "lastModified": 2_000,
                "treeState": "clean",
                "diskBytes": 128
            }
        ]);
        let project_issues = json!([
            {
                "identifier": "#244",
                "title": "Old completed issue",
                "state": "Done",
                "url": "https://github.com/Alive24/shea-symphony/issues/244",
                "updatedAt": "2026-06-03T09:00:00Z"
            }
        ]);

        let completed = completed_issue_worktrees(&snapshot, &issue_worktrees, &project_issues);
        let rows = completed.as_array().unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["state"], "Done");
        assert!(rows[0]["lastProgressAt"].is_null());
        assert_eq!(rows[0]["lastProgressSource"], "unavailable");
        assert_eq!(rows[0]["projectUpdatedAt"], "2026-06-03T09:00:00Z");
        assert_eq!(rows[0]["lastModified"], 2_000);
        assert_eq!(
            rows[0]["timestampSources"]["projectUpdatedAt"]["meaning"],
            "Broad Project/GitHub issue readback timestamp retained as metadata only; it must not drive Last Progress."
        );
    }

    #[test]
    fn completed_session_without_title_still_needs_project_readback() {
        let snapshot = json!({
            "sessions": [
                {
                    "status": "completed",
                    "issue_identifier": "#326",
                    "issue_title": null,
                    "lane": "main",
                    "started_at_ms": 900,
                    "updated_at_ms": 1_500
                },
                {
                    "status": "completed",
                    "issue_identifier": "#327",
                    "issue_title": "Known completed issue",
                    "lane": "main",
                    "started_at_ms": 900,
                    "updated_at_ms": 1_500
                }
            ]
        });

        let registry_issues = session_registry_issue_refs(&snapshot);

        assert!(!registry_issues.contains_key("#326"));
        assert!(registry_issues.contains_key("#327"));
    }

    #[test]
    fn session_registry_progress_takes_precedence_over_project_readback_metadata() {
        let snapshot = json!({
            "sessions": [
                {
                    "status": "completed",
                    "issue_identifier": "#251",
                    "issue_title": "Session-backed completed issue",
                    "lane": "main",
                    "started_at_ms": 900,
                    "updated_at_ms": 1_500,
                    "evidence": "Main handoff evidence"
                }
            ]
        });
        let issue_worktrees = json!([
            {
                "issue": "#251",
                "path": "/tmp/issue-251",
                "branch": "feature/issue-251",
                "head": "def5678",
                "createdAt": 1_000,
                "lastModified": 3_000,
                "treeState": "dirty",
                "diskBytes": 256
            }
        ]);
        let project_issues = json!([
            {
                "identifier": "#251",
                "title": "Project title",
                "state": "Done",
                "url": "https://github.com/Alive24/shea-symphony/issues/251",
                "updatedAt": "2026-06-03T09:00:00Z"
            }
        ]);

        let completed = completed_issue_worktrees(&snapshot, &issue_worktrees, &project_issues);
        let rows = completed.as_array().unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["lastProgressAt"], 1_500);
        assert_eq!(
            rows[0]["lastProgressSource"],
            "session_registry.updated_at_ms"
        );
        assert_eq!(rows[0]["projectUpdatedAt"], "2026-06-03T09:00:00Z");
        assert_eq!(rows[0]["lastModified"], 3_000);
    }

    #[test]
    fn project_readback_cache_does_not_create_handoff_progress() {
        let snapshot = json!({
            "sessions": [
                {
                    "status": "recorded",
                    "session_source": "project_readback_cache",
                    "issue_identifier": "#248",
                    "issue_title": "Cached project readback",
                    "project_state": "Done",
                    "project_url": "https://github.com/Alive24/shea-symphony/issues/248",
                    "lane": "project",
                    "started_at_ms": 4_000,
                    "updated_at_ms": 5_000
                }
            ]
        });
        let issue_worktrees = json!([
            {
                "issue": "#248",
                "path": "/tmp/issue-248",
                "branch": "feature/issue-248",
                "head": "feed248",
                "createdAt": 1_000,
                "lastModified": 2_000,
                "treeState": "clean",
                "diskBytes": 512
            }
        ]);

        let completed = completed_issue_worktrees(&snapshot, &issue_worktrees, &json!([]));
        let rows = completed.as_array().unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["state"], "Done");
        assert!(rows[0]["lastProgressAt"].is_null());
        assert_eq!(rows[0]["lastProgressSource"], "unavailable");
        assert_eq!(rows[0]["artifactSource"], "project_readback_cache");
        assert_eq!(rows[0]["lastModified"], 2_000);
    }

    #[test]
    fn finds_rollout_jsonl_by_session_id_under_codex_sessions_tree() {
        let root =
            std::env::temp_dir().join(format!("shea-transcript-test-{}", unix_timestamp_ms()));
        let day = root.join("2026").join("06").join("02");
        fs::create_dir_all(&day).unwrap();
        let wanted = day.join("rollout-2026-06-02T17-30-00-019f-session-match.jsonl");
        let ignored = day.join("rollout-2026-06-02T17-31-00-other.jsonl");
        fs::write(&wanted, "{}\n").unwrap();
        fs::write(&ignored, "{}\n").unwrap();

        let matches = find_rollout_jsonl(&root, "019f-session-match", 10);

        assert_eq!(matches, vec![wanted]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn splits_app_server_thread_id_from_composite_session_id() {
        let needles = session_store_needles(
            "019e8963-d029-7a91-b144-63ee3fbe71a9-019e8963-d0df-7451-bc85-19cacf85d5a9",
        );

        assert!(needles.contains(&"019e8963-d029-7a91-b144-63ee3fbe71a9".to_string()));
        assert!(needles.contains(&"019e8963-d0df-7451-bc85-19cacf85d5a9".to_string()));
    }

    #[test]
    fn transcript_candidates_include_app_server_protocol_fallback_by_issue() {
        let root =
            std::env::temp_dir().join(format!("shea-transcript-test-{}", unix_timestamp_ms()));
        let logs_root = root.join("logs");
        let app_server_root = logs_root.join("app-server");
        fs::create_dir_all(&app_server_root).unwrap();
        let protocol = app_server_root.join("414-1780421480389.protocol.jsonl");
        fs::write(&protocol, "{}\n").unwrap();
        let snapshot = json!({
            "event_log_path": logs_root.join("shea-symphony.jsonl").display().to_string(),
            "sessions": [
                {
                    "issue_identifier": "#414",
                    "session_id": "019e8963-d029-7a91-b144-63ee3fbe71a9-019e8963-d0df-7451-bc85-19cacf85d5a9",
                    "run_id": "20260602T1730Z-issue414-main-fddb",
                    "log_path": null
                }
            ]
        });

        let candidates = transcript_candidates(&snapshot, Some("#414"), None);
        let expected_path = protocol.display().to_string();

        assert!(candidates.iter().any(|candidate| {
            candidate.get("path").and_then(Value::as_str) == Some(expected_path.as_str())
        }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_conversation_summary_extracts_app_server_deep_link_and_message_times() {
        let path = PathBuf::from("/tmp/430-1780519230584.protocol.jsonl");
        let content = [
            json!({
                "direction": "stdout",
                "line": json!({
                    "id": 2,
                    "method": "thread/start",
                    "result": {
                        "thread": {
                            "id": "019e8f37-5cab-74f3-9933-93e3809396e5",
                            "sessionId": "019e8f37-5cab-74f3-9933-93e3809396e5"
                        }
                    }
                }).to_string()
            })
            .to_string(),
            json!({
                "direction": "stdin",
                "line": json!({
                    "method": "turn/start",
                    "threadId": "019e8f37-5cab-74f3-9933-93e3809396e5",
                    "turnId": "019e8f37-5ddf-70a3-a616-85983455519c",
                    "completedAtMs": 1_780_519_235_370_u64,
                    "params": {
                        "input": [{ "text": "Implement #430." }]
                    }
                }).to_string()
            })
            .to_string(),
            json!({
                "direction": "stdout",
                "line": json!({
                    "method": "turn/started",
                    "params": {
                        "threadId": "019e8f37-5cab-74f3-9933-93e3809396e5",
                        "turn": {
                            "id": "019e8f37-5ddf-70a3-a616-85983455519c",
                            "startedAt": 1_780_519_230_u64
                        }
                    }
                }).to_string()
            })
            .to_string(),
            json!({
                "direction": "stdout",
                "line": json!({
                    "method": "item/completed",
                    "params": {
                        "threadId": "019e8f37-5cab-74f3-9933-93e3809396e5",
                        "turnId": "019e8f37-5ddf-70a3-a616-85983455519c",
                        "completedAtMs": 1_780_519_567_265_u64,
                        "item": {
                            "type": "agentMessage",
                            "text": "Done.",
                            "phase": "final_answer"
                        }
                    }
                }).to_string()
            })
            .to_string(),
        ]
        .join("\n");

        let summary = codex_conversation_summary(&content, &path);

        assert_eq!(summary["threadId"], "019e8f37-5cab-74f3-9933-93e3809396e5");
        assert_eq!(summary["turnId"], "019e8f37-5ddf-70a3-a616-85983455519c");
        assert_eq!(
            summary["deepLink"],
            "codex://threads/019e8f37-5cab-74f3-9933-93e3809396e5"
        );
        assert_eq!(summary["lastUserMessageAt"], 1_780_519_230_000_u64);
        assert_eq!(summary["lastAssistantMessageAt"], 1_780_519_567_265_u64);
        assert_eq!(summary["messageCounts"]["user"], 1);
        assert_eq!(summary["messageCounts"]["assistant"], 1);
    }

    #[test]
    fn codex_conversation_summary_extracts_rollout_timestamps() {
        let path = PathBuf::from(
            "/tmp/rollout-2026-06-03T21-40-30-019e8f37-5cab-74f3-9933-93e3809396e5.jsonl",
        );
        let content = [
            json!({
                "timestamp": "2026-06-03T20:40:30.000Z",
                "type": "session_meta",
                "payload": {
                    "id": "019e8f37-5cab-74f3-9933-93e3809396e5"
                }
            })
            .to_string(),
            json!({
                "timestamp": "2026-06-03T20:40:35.000Z",
                "type": "event_msg",
                "payload": {
                    "type": "user_message",
                    "message": "Implement #430."
                }
            })
            .to_string(),
            json!({
                "timestamp": "2026-06-03T20:46:07.000Z",
                "type": "event_msg",
                "payload": {
                    "type": "agent_message",
                    "phase": "final_answer",
                    "message": "Done."
                }
            })
            .to_string(),
        ]
        .join("\n");

        let summary = codex_conversation_summary(&content, &path);

        assert_eq!(
            summary["deepLink"],
            "codex://threads/019e8f37-5cab-74f3-9933-93e3809396e5"
        );
        assert_eq!(summary["lastUserMessageAt"], "2026-06-03T20:40:35.000Z");
        assert_eq!(
            summary["lastAssistantMessageAt"],
            "2026-06-03T20:46:07.000Z"
        );
    }
}
