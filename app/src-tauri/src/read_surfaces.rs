use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cli::{
    command_summary_value, parse_json_output, pending_result, run_shea_read, shea_command,
    timestamp_iso_like, CommandRun, DEFAULT_WORKFLOW_PATH,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewOptions {
    force: Option<bool>,
    scope: Option<String>,
}

#[tauri::command]
pub async fn get_runtime_snapshot() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let output = shea_command(&["status", "show", DEFAULT_WORKFLOW_PATH, "--json"])
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
pub async fn get_operator_overview(options: Option<OverviewOptions>) -> Result<Value, String> {
    let scope = options
        .as_ref()
        .and_then(|options| options.scope.as_deref())
        .unwrap_or("full")
        .to_string();
    tauri::async_runtime::spawn_blocking(move || build_operator_overview(&scope))
        .await
        .map_err(|error| format!("operator overview task failed: {error}"))?
}

#[tauri::command]
pub async fn get_read_surface(name: String, _force: Option<bool>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || build_read_surface(&name))
        .await
        .map_err(|error| format!("read surface task failed: {error}"))?
}

fn build_read_surface(name: &str) -> Result<Value, String> {
    if name == "githubQueue" {
        return Ok(surface_payload(
            "githubQueue",
            pending_result(
                &["autopilot", "plan", DEFAULT_WORKFLOW_PATH, "--json"],
                "Project queue is represented through CLI autopilot parked queues in Tauri mode.",
            ),
            Value::Null,
            String::new(),
        ));
    }

    let args = read_surface_args(name).ok_or_else(|| format!("unknown read surface: {name}"))?;
    let result = run_shea_read(&args);
    Ok(surface_payload(
        name,
        command_summary_value(&result.summary),
        parse_json_output(&result.stdout),
        if name == "sessions" {
            result.stdout.trim().to_string()
        } else {
            String::new()
        },
    ))
}

fn build_operator_overview(scope: &str) -> Result<Value, String> {
    let generated_at = timestamp_iso_like();
    let github_queue_command = pending_result(
        &["autopilot", "plan", DEFAULT_WORKFLOW_PATH, "--json"],
        "Project queue is represented through CLI autopilot parked queues in Tauri mode.",
    );

    if scope == "fast" {
        let commands = json!({
            "autopilot": pending_result(&["autopilot", "plan", DEFAULT_WORKFLOW_PATH, "--json"], "Deferred to full overview."),
            "doctor": pending_result(&["doctor", DEFAULT_WORKFLOW_PATH, "--json"], "Deferred to full overview."),
            "review": pending_result(&["review", "status", DEFAULT_WORKFLOW_PATH, "--json"], "Deferred to full overview."),
            "skills": pending_result(&["skills", "status", DEFAULT_WORKFLOW_PATH, "--json"], "Deferred to background read."),
            "sessions": pending_result(&["session", "list", DEFAULT_WORKFLOW_PATH], "Deferred to background read."),
            "local": pending_result(&["status", "show", DEFAULT_WORKFLOW_PATH, "--json"], "Deferred to background read."),
            "githubQueue": github_queue_command,
        });
        return Ok(json!({
            "generatedAt": generated_at,
            "workflowPath": DEFAULT_WORKFLOW_PATH,
            "scope": "fast",
            "commands": commands,
            "autopilot": Value::Null,
            "doctor": Value::Null,
            "review": Value::Null,
            "skills": Value::Null,
            "sessionsText": "",
            "localStatus": Value::Null,
            "githubQueue": Value::Null,
            "healthy": true,
        }));
    }

    let runtime = run_shea_read(&read_surface_args("local").unwrap_or_default());
    let autopilot = run_shea_read(&read_surface_args("autopilot").unwrap_or_default());
    let doctor = run_shea_read(&read_surface_args("doctor").unwrap_or_default());
    let review = run_shea_read(&read_surface_args("review").unwrap_or_default());
    let skills = run_shea_read(&read_surface_args("skills").unwrap_or_default());
    let sessions = run_shea_read(&read_surface_args("sessions").unwrap_or_default());
    let healthy = [
        autopilot.summary.ok,
        doctor.summary.ok,
        review.summary.ok,
        skills.summary.ok,
    ]
    .iter()
    .any(|ok| *ok);

    Ok(json!({
        "generatedAt": generated_at,
        "workflowPath": DEFAULT_WORKFLOW_PATH,
        "commands": {
            "autopilot": command_summary_value(&autopilot.summary),
            "doctor": command_summary_value(&doctor.summary),
            "review": command_summary_value(&review.summary),
            "skills": command_summary_value(&skills.summary),
            "sessions": command_summary_value(&sessions.summary),
            "local": command_summary_value(&runtime.summary),
            "githubQueue": github_queue_command,
        },
        "autopilot": parse_json_output(&autopilot.stdout),
        "doctor": parse_json_output(&doctor.stdout),
        "review": parse_json_output(&review.stdout),
        "skills": parse_json_output(&skills.stdout),
        "sessionsText": sessions.stdout.trim(),
        "localStatus": runtime_status_summary(&runtime),
        "githubQueue": Value::Null,
        "healthy": healthy,
    }))
}

fn read_surface_args(name: &str) -> Option<Vec<String>> {
    match name {
        "autopilot" => Some(vec![
            "autopilot".into(),
            "plan".into(),
            DEFAULT_WORKFLOW_PATH.into(),
            "--json".into(),
        ]),
        "doctor" => Some(vec![
            "doctor".into(),
            DEFAULT_WORKFLOW_PATH.into(),
            "--json".into(),
        ]),
        "review" => Some(vec![
            "review".into(),
            "status".into(),
            DEFAULT_WORKFLOW_PATH.into(),
            "--json".into(),
        ]),
        "skills" => Some(vec![
            "skills".into(),
            "status".into(),
            DEFAULT_WORKFLOW_PATH.into(),
            "--json".into(),
        ]),
        "sessions" => Some(vec![
            "session".into(),
            "list".into(),
            DEFAULT_WORKFLOW_PATH.into(),
        ]),
        "local" => Some(vec![
            "status".into(),
            "show".into(),
            DEFAULT_WORKFLOW_PATH.into(),
            "--json".into(),
        ]),
        _ => None,
    }
}

fn runtime_status_summary(result: &CommandRun) -> Value {
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
    json!({
        "source": "shea-symphony status show --json",
        "runningCount": running_count,
        "plannedCount": planned_count,
        "retryingCount": retrying_count,
        "sessionCount": session_count,
        "integrationGapCount": integration_gap_count,
        "eventLogPath": snapshot.get("event_log_path").cloned().unwrap_or(Value::Null),
        "snapshot": snapshot,
    })
}

fn surface_payload(name: &str, command: Value, parsed: Value, text: String) -> Value {
    json!({
        "name": name,
        "generatedAt": timestamp_iso_like(),
        "workflowPath": DEFAULT_WORKFLOW_PATH,
        "command": command,
        "parsed": parsed,
        "text": text,
    })
}
