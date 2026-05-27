use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

const DEFAULT_WORKFLOW_PATH: &str = "workflows/shea-symphony.md";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoloopOptions {
    workflow_path: Option<String>,
    max_iterations: Option<usize>,
    once: Option<bool>,
    write: Option<bool>,
    signal_format: Option<String>,
    poll_interval_ms: Option<u64>,
    main_max_concurrent: Option<usize>,
    review_max_concurrent: Option<usize>,
    merge_max_concurrent: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoopStateSnapshot {
    running: bool,
    stopping: bool,
    pid: Option<u32>,
    mode: String,
    workflow_path: String,
    started_at_ms: Option<u128>,
    stopped_at_ms: Option<u128>,
    exit_code: Option<i32>,
    error: Option<String>,
    lanes: BTreeMap<String, LaneSnapshot>,
    recent_lines: Vec<AutoloopLine>,
}

impl Default for LoopStateSnapshot {
    fn default() -> Self {
        Self {
            running: false,
            stopping: false,
            pid: None,
            mode: "dry-run".into(),
            workflow_path: DEFAULT_WORKFLOW_PATH.into(),
            started_at_ms: None,
            stopped_at_ms: None,
            exit_code: None,
            error: None,
            lanes: default_lanes(),
            recent_lines: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaneSnapshot {
    lane: String,
    status: String,
    action: Option<String>,
    selected: Option<String>,
    target: Option<String>,
    max_concurrent: Option<usize>,
    recover: Option<bool>,
    updated_at_ms: Option<u128>,
    latest_line: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoloopLine {
    stream: String,
    line: String,
    at_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoloopStarted {
    pid: u32,
    command: Vec<String>,
    mode: String,
    workflow_path: String,
    at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoloopStopped {
    pid: Option<u32>,
    exit_code: Option<i32>,
    at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoloopError {
    message: String,
    at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverviewOptions {
    force: Option<bool>,
    scope: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandSummary {
    ok: bool,
    args: Vec<String>,
    exit_code: Option<i32>,
    signal: Option<String>,
    timed_out: bool,
    duration_ms: u128,
    stderr: String,
    stdout_preview: String,
}

#[derive(Debug, Clone)]
struct CommandRun {
    summary: CommandSummary,
    stdout: String,
}

#[derive(Debug, Default)]
struct LoopRuntime {
    state: LoopStateSnapshot,
}

#[derive(Clone, Default)]
struct LoopManager {
    inner: Arc<Mutex<LoopRuntime>>,
}

#[tauri::command]
fn get_loop_state(manager: State<'_, LoopManager>) -> Result<LoopStateSnapshot, String> {
    Ok(manager
        .inner
        .lock()
        .map_err(|error| error.to_string())?
        .state
        .clone())
}

#[tauri::command]
async fn get_runtime_snapshot() -> Result<serde_json::Value, String> {
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
async fn get_operator_overview(options: Option<OverviewOptions>) -> Result<Value, String> {
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
async fn get_read_surface(name: String, _force: Option<bool>) -> Result<Value, String> {
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

#[tauri::command]
fn start_autoloop(
    app: AppHandle,
    manager: State<'_, LoopManager>,
    options: Option<AutoloopOptions>,
) -> Result<LoopStateSnapshot, String> {
    let options = options.unwrap_or(AutoloopOptions {
        workflow_path: None,
        max_iterations: Some(1),
        once: None,
        write: Some(false),
        signal_format: Some("json".into()),
        poll_interval_ms: None,
        main_max_concurrent: None,
        review_max_concurrent: None,
        merge_max_concurrent: None,
    });
    let workflow_path = options
        .workflow_path
        .clone()
        .unwrap_or_else(|| DEFAULT_WORKFLOW_PATH.into());
    let write = options.write.unwrap_or(false);
    let mode = if write { "write" } else { "dry-run" }.to_string();
    let mut args = vec![
        "autopilot".to_string(),
        "loop".to_string(),
        workflow_path.clone(),
    ];
    if options.once.unwrap_or(false) {
        args.push("--once".into());
    } else {
        args.push("--max-iterations".into());
        args.push(options.max_iterations.unwrap_or(1).max(1).to_string());
    }
    args.push(if write { "--write" } else { "--dry-run" }.into());
    if options.signal_format.as_deref().unwrap_or("json") != "plain" {
        args.push("--event-json".into());
    }
    if let Some(value) = options.poll_interval_ms {
        args.push("--poll-interval-ms".into());
        args.push(value.max(1).to_string());
    }
    if let Some(value) = options.main_max_concurrent {
        args.push("--main-max-concurrent".into());
        args.push(value.max(1).to_string());
    }
    if let Some(value) = options.review_max_concurrent {
        args.push("--review-max-concurrent".into());
        args.push(value.max(1).to_string());
    }
    if let Some(value) = options.merge_max_concurrent {
        args.push("--merge-max-concurrent".into());
        args.push(value.max(1).to_string());
    }

    {
        let mut runtime = manager.inner.lock().map_err(|error| error.to_string())?;
        if runtime.state.running {
            return Err(format!(
                "autoloop already running with pid {}",
                runtime
                    .state
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "unknown".into())
            ));
        }
        runtime.state = LoopStateSnapshot {
            running: true,
            stopping: false,
            pid: None,
            mode: mode.clone(),
            workflow_path: workflow_path.clone(),
            started_at_ms: Some(now_ms()),
            stopped_at_ms: None,
            exit_code: None,
            error: None,
            lanes: default_lanes(),
            recent_lines: Vec::new(),
        };
    }

    let mut command = shea_command(&args.iter().map(String::as_str).collect::<Vec<_>>());
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let command_for_event = command_preview(&args);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            record_error(&app, &manager, format!("failed to start autoloop: {error}"))?;
            return Err(format!("failed to start autoloop: {error}"));
        }
    };

    let pid = child.id();
    {
        let mut runtime = manager.inner.lock().map_err(|error| error.to_string())?;
        runtime.state.pid = Some(pid);
    }
    let started = AutoloopStarted {
        pid,
        command: command_for_event,
        mode: mode.clone(),
        workflow_path: workflow_path.clone(),
        at_ms: now_ms(),
    };
    emit(&app, "autoloop:started", &started);

    if let Some(stdout) = child.stdout.take() {
        spawn_line_reader(app.clone(), manager.inner.clone(), "stdout", stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_line_reader(app.clone(), manager.inner.clone(), "stderr", stderr);
    }

    let app_for_wait = app.clone();
    let manager_for_wait = manager.inner.clone();
    thread::spawn(move || {
        let status = child.wait();
        let (exit_code, wait_error) = match status {
            Ok(status) => (status.code(), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let stopped = AutoloopStopped {
            pid: Some(pid),
            exit_code,
            at_ms: now_ms(),
        };
        if let Ok(mut runtime) = manager_for_wait.lock() {
            runtime.state.running = false;
            runtime.state.stopping = false;
            runtime.state.pid = None;
            runtime.state.stopped_at_ms = Some(stopped.at_ms);
            runtime.state.exit_code = stopped.exit_code;
            runtime.state.error = wait_error;
            if runtime.state.error.is_none() && stopped.exit_code.is_some_and(|code| code != 0) {
                runtime.state.error = Some(format!(
                    "autoloop exited with status {}",
                    stopped.exit_code.unwrap_or_default()
                ));
            }
        }
        emit(&app_for_wait, "autoloop:stopped", &stopped);
        if stopped.exit_code.is_some_and(|code| code != 0) {
            emit(
                &app_for_wait,
                "autoloop:error",
                &AutoloopError {
                    message: format!(
                        "autoloop exited with status {}",
                        stopped.exit_code.unwrap_or_default()
                    ),
                    at_ms: stopped.at_ms,
                },
            );
        }
    });

    get_loop_state(manager)
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

fn run_shea_read(args: &[String]) -> CommandRun {
    let started_at = Instant::now();
    let string_args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let mut command = shea_command(&string_args);
    match command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => match child.wait_with_output() {
            Ok(output) => command_run_from_output(args, started_at, output, false),
            Err(error) => command_run_from_error(args, started_at, error.to_string()),
        },
        Err(error) => command_run_from_error(args, started_at, error.to_string()),
    }
}

fn command_run_from_output(
    args: &[String],
    started_at: Instant,
    output: std::process::Output,
    timed_out: bool,
) -> CommandRun {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if timed_out {
        stderr = if stderr.is_empty() {
            "read command timed out".into()
        } else {
            format!("read command timed out\n{stderr}")
        };
    }
    CommandRun {
        summary: CommandSummary {
            ok: output.status.success() && !timed_out,
            args: args.to_vec(),
            exit_code: output.status.code(),
            signal: if timed_out {
                Some("timeout".into())
            } else {
                None
            },
            timed_out,
            duration_ms: started_at.elapsed().as_millis(),
            stderr,
            stdout_preview: stdout.trim().chars().take(6000).collect(),
        },
        stdout,
    }
}

fn command_run_from_error(args: &[String], started_at: Instant, error: String) -> CommandRun {
    CommandRun {
        summary: CommandSummary {
            ok: false,
            args: args.to_vec(),
            exit_code: None,
            signal: None,
            timed_out: false,
            duration_ms: started_at.elapsed().as_millis(),
            stderr: error,
            stdout_preview: String::new(),
        },
        stdout: String::new(),
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

fn pending_result(args: &[&str], reason: &str) -> Value {
    json!({
        "ok": false,
        "pending": true,
        "args": args,
        "exitCode": Value::Null,
        "signal": Value::Null,
        "timedOut": false,
        "durationMs": 0,
        "stderr": reason,
        "stdoutPreview": "",
    })
}

fn command_summary_value(summary: &CommandSummary) -> Value {
    serde_json::to_value(summary).unwrap_or_else(|_| json!({ "ok": false }))
}

fn parse_json_output(stdout: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or(Value::Null)
}

fn timestamp_iso_like() -> u128 {
    now_ms()
}

#[tauri::command]
fn stop_autoloop(
    app: AppHandle,
    manager: State<'_, LoopManager>,
) -> Result<LoopStateSnapshot, String> {
    let pid = {
        let mut runtime = manager.inner.lock().map_err(|error| error.to_string())?;
        let Some(pid) = runtime.state.pid else {
            runtime.state.running = false;
            runtime.state.stopping = false;
            return Ok(runtime.state.clone());
        };
        runtime.state.stopping = true;
        pid
    };

    #[cfg(unix)]
    let stop_result = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
    #[cfg(not(unix))]
    let stop_result = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status();

    if let Err(error) = stop_result {
        record_error(
            &app,
            &manager,
            format!("failed to signal autoloop stop: {error}"),
        )?;
        return Err(format!("failed to signal autoloop stop: {error}"));
    }

    get_loop_state(manager)
}

fn shea_command(args: &[&str]) -> Command {
    let repo_root = repo_root();
    let binary = repo_root.join("target").join("debug").join("shea-symphony");
    if binary.exists() {
        let mut command = Command::new(binary);
        command.args(args).current_dir(repo_root);
        command
    } else {
        let mut command = Command::new("cargo");
        command
            .args(["run", "--quiet", "--"])
            .args(args)
            .current_dir(repo_root);
        command
    }
}

fn command_preview(args: &[String]) -> Vec<String> {
    let repo_root = repo_root();
    let binary = repo_root.join("target").join("debug").join("shea-symphony");
    if binary.exists() {
        let mut preview = vec![binary.display().to_string()];
        preview.extend(args.iter().cloned());
        preview
    } else {
        let mut preview = vec!["cargo".into(), "run".into(), "--quiet".into(), "--".into()];
        preview.extend(args.iter().cloned());
        preview
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn spawn_line_reader<R: std::io::Read + Send + 'static>(
    app: AppHandle,
    manager: Arc<Mutex<LoopRuntime>>,
    stream: &'static str,
    reader: R,
) {
    thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines().map_while(Result::ok) {
            let event =
                parse_autoloop_event(&line).or_else(|| parse_autoloop_text_event(stream, &line));
            let payload = AutoloopLine {
                stream: stream.into(),
                line: line.clone(),
                at_ms: now_ms(),
                event: event.clone(),
            };
            let mut snapshot = None;
            if let Ok(mut runtime) = manager.lock() {
                push_recent_line(&mut runtime.state, payload.clone());
                if let Some(lane) = parse_autoloop_lane_event(event.as_ref(), payload.at_ms)
                    .or_else(|| parse_autoloop_lane(&line, payload.at_ms))
                {
                    runtime.state.lanes.insert(lane.lane.clone(), lane.clone());
                    emit(&app, "autoloop:lane", &lane);
                }
                if apply_autoloop_event(&mut runtime.state, event.as_ref(), payload.at_ms)
                    || apply_autoloop_result(&mut runtime.state, &line)
                    || apply_autoloop_stopped(&mut runtime.state, &line, payload.at_ms)
                {
                    snapshot = Some(runtime.state.clone());
                }
            }
            emit(&app, "autoloop:line", &payload);
            if let Some(snapshot) = snapshot {
                emit(&app, "autoloop:snapshot", &snapshot);
            }
        }
    });
}

fn push_recent_line(state: &mut LoopStateSnapshot, line: AutoloopLine) {
    state.recent_lines.push(line);
    if state.recent_lines.len() > 200 {
        let overflow = state.recent_lines.len() - 200;
        state.recent_lines.drain(0..overflow);
    }
}

fn parse_autoloop_event(line: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("source").and_then(Value::as_str) != Some("shea-symphony") {
        return None;
    }
    value.get("event").and_then(Value::as_str)?;
    Some(value)
}

fn parse_autoloop_text_event(stream: &str, line: &str) -> Option<Value> {
    if line.trim().is_empty() {
        return None;
    }
    Some(json!({
        "schema_version": 1,
        "source": "shea-symphony",
        "event": "autopilot_cli_line",
        "payload": {
            "stream": stream,
            "kind": cli_line_kind(line),
            "raw": line,
            "fields": parse_cli_line_fields(line),
        }
    }))
}

fn parse_autoloop_lane_event(event: Option<&Value>, at_ms: u128) -> Option<LaneSnapshot> {
    let event = event?;
    if event.get("event").and_then(Value::as_str)? != "autopilot_loop_lane" {
        return None;
    }
    let payload = event.get("payload")?;
    let lane = string_json_field(payload, "lane")?;
    Some(LaneSnapshot {
        lane,
        status: string_json_field(payload, "status").unwrap_or_else(|| "unknown".into()),
        action: optional_json_field(payload, "action"),
        selected: selected_issue_field(payload),
        target: optional_json_field(payload, "target_state")
            .or_else(|| optional_json_field(payload, "target")),
        max_concurrent: payload
            .get("max_concurrent")
            .or_else(|| payload.get("maxConcurrent"))
            .and_then(Value::as_u64)
            .map(|value| value as usize),
        recover: payload.get("recover").and_then(Value::as_bool),
        updated_at_ms: Some(at_ms),
        latest_line: Some(event.to_string()),
    })
}

fn cli_line_kind(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.starts_with("Latest:") {
        return "latest".into();
    }
    trimmed
        .split_once(['=', ' '])
        .map(|(kind, _)| kind.trim_end_matches(':').to_string())
        .filter(|kind| !kind.is_empty())
        .unwrap_or_else(|| "line".into())
}

fn parse_cli_line_fields(line: &str) -> Value {
    let trimmed = line.trim();
    if trimmed.starts_with("Latest:") {
        return parse_latest_line(trimmed);
    }
    let mut fields = serde_json::Map::new();
    for (key, value) in parse_shell_like_key_values(trimmed) {
        fields.insert(key, Value::String(value));
    }
    Value::Object(fields)
}

fn parse_latest_line(line: &str) -> Value {
    let parts = line
        .strip_prefix("Latest:")
        .unwrap_or(line)
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>();
    let mut fields = serde_json::Map::new();
    for (key, value) in ["lane", "issue", "status", "action", "title"]
        .into_iter()
        .zip(parts.iter().copied())
    {
        if !value.is_empty() {
            fields.insert(key.into(), Value::String(value.into()));
        }
    }
    for part in parts.into_iter().skip(5) {
        if let Some((key, value)) = part.split_once('=') {
            fields.insert(key.into(), Value::String(value.into()));
        }
    }
    Value::Object(fields)
}

fn parse_shell_like_key_values(line: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for token in shell_like_tokens(line) {
        if let Some((key, value)) = token.split_once('=') {
            fields.insert(key.to_string(), value.to_string());
        }
    }
    fields
}

fn shell_like_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(active), current_ch) if current_ch == active => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, ch) if ch.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (_, ch) => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn parse_autoloop_lane(line: &str, at_ms: u128) -> Option<LaneSnapshot> {
    if !line.starts_with("autopilot_loop_lane ") {
        return None;
    }
    let fields = parse_key_values(line);
    let lane = fields.get("lane")?.to_string();
    Some(LaneSnapshot {
        lane: lane.clone(),
        status: fields
            .get("status")
            .cloned()
            .unwrap_or_else(|| "unknown".into()),
        action: optional_field(&fields, "action"),
        selected: optional_field(&fields, "selected"),
        target: optional_field(&fields, "target"),
        max_concurrent: fields
            .get("max_concurrent")
            .and_then(|value| value.parse::<usize>().ok()),
        recover: fields
            .get("recover")
            .and_then(|value| value.parse::<bool>().ok()),
        updated_at_ms: Some(at_ms),
        latest_line: Some(line.into()),
    })
}

fn apply_autoloop_result(state: &mut LoopStateSnapshot, line: &str) -> bool {
    if !line.starts_with("autopilot_loop_result ") {
        return false;
    }
    let fields = parse_key_values(line);
    if let Some(mode) = fields.get("mode") {
        state.mode = mode.clone();
    }
    let lane_concurrency = [
        ("main", "main_max_concurrent"),
        ("review", "review_max_concurrent"),
        ("merge", "merge_max_concurrent"),
    ];
    for (lane, field) in lane_concurrency {
        if let Some(value) = fields
            .get(field)
            .and_then(|value| value.parse::<usize>().ok())
        {
            state
                .lanes
                .entry(lane.into())
                .or_insert_with(|| default_lane(lane))
                .max_concurrent = Some(value);
        }
    }
    true
}

fn apply_autoloop_event(state: &mut LoopStateSnapshot, event: Option<&Value>, at_ms: u128) -> bool {
    let Some(event) = event else {
        return false;
    };
    let Some(event_name) = event.get("event").and_then(Value::as_str) else {
        return false;
    };
    let payload = event.get("payload").unwrap_or(&Value::Null);
    match event_name {
        "autopilot_loop_status" => {
            if let Some(mode) = string_json_field(payload, "mode") {
                state.mode = mode;
            }
            apply_json_settings(state, payload.get("settings"));
            if let Some(lanes) = payload.get("lane_activity").and_then(Value::as_array) {
                for lane_payload in lanes {
                    if let Some(lane) = parse_status_lane_activity(lane_payload, at_ms) {
                        state.lanes.insert(lane.lane.clone(), lane);
                    }
                }
            }
            true
        }
        "autopilot_loop_iteration" => {
            if let Some(mode) = string_json_field(payload, "mode") {
                state.mode = mode;
            }
            apply_json_settings(state, payload.get("settings"));
            true
        }
        "autopilot_loop_result" => {
            if let Some(mode) = string_json_field(payload, "mode") {
                state.mode = mode;
            }
            apply_json_settings(state, payload.get("settings"));
            true
        }
        "autopilot_loop_stopped" => {
            state.running = false;
            state.stopping = false;
            state.pid = None;
            state.stopped_at_ms = Some(at_ms);
            true
        }
        _ => false,
    }
}

fn parse_status_lane_activity(payload: &Value, at_ms: u128) -> Option<LaneSnapshot> {
    let lane = string_json_field(payload, "lane")?;
    Some(LaneSnapshot {
        lane,
        status: string_json_field(payload, "status").unwrap_or_else(|| "unknown".into()),
        action: optional_json_field(payload, "action"),
        selected: selected_issue_field(payload),
        target: optional_json_field(payload, "target_state")
            .or_else(|| optional_json_field(payload, "target")),
        max_concurrent: None,
        recover: None,
        updated_at_ms: Some(at_ms),
        latest_line: Some(payload.to_string()),
    })
}

fn apply_json_settings(state: &mut LoopStateSnapshot, settings: Option<&Value>) {
    let Some(settings) = settings else {
        return;
    };
    let lane_concurrency = [
        ("main", "main_max_concurrent"),
        ("review", "review_max_concurrent"),
        ("merge", "merge_max_concurrent"),
    ];
    for (lane, field) in lane_concurrency {
        if let Some(value) = settings
            .get(field)
            .or_else(|| settings.get(&snake_to_camel(field)))
            .and_then(Value::as_u64)
        {
            state
                .lanes
                .entry(lane.into())
                .or_insert_with(|| default_lane(lane))
                .max_concurrent = Some(value as usize);
        }
    }
}

fn apply_autoloop_stopped(state: &mut LoopStateSnapshot, line: &str, at_ms: u128) -> bool {
    if !line.starts_with("autopilot_loop=stopped ") {
        return false;
    }
    state.running = false;
    state.stopping = false;
    state.pid = None;
    state.stopped_at_ms = Some(at_ms);
    true
}

fn optional_field(fields: &BTreeMap<String, String>, key: &str) -> Option<String> {
    fields
        .get(key)
        .filter(|value| !value.is_empty() && value.as_str() != "none")
        .cloned()
}

fn string_json_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            payload
                .get(&snake_to_camel(key))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn optional_json_field(payload: &Value, key: &str) -> Option<String> {
    string_json_field(payload, key).filter(|value| !value.is_empty() && value != "none")
}

fn selected_issue_field(payload: &Value) -> Option<String> {
    optional_json_field(payload, "selected").or_else(|| {
        payload
            .get("selected_issue")
            .or_else(|| payload.get("selectedIssue"))
            .and_then(|issue| {
                issue
                    .get("identifier")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| issue.as_str().map(str::to_string))
            })
            .filter(|value| !value.is_empty() && value != "none")
    })
}

fn snake_to_camel(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut uppercase_next = false;
    for ch in value.chars() {
        if ch == '_' {
            uppercase_next = true;
        } else if uppercase_next {
            output.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            output.push(ch);
        }
    }
    output
}

fn parse_key_values(line: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for token in line.split_whitespace().skip(1) {
        if let Some((key, value)) = token.split_once('=') {
            fields.insert(key.to_string(), unquote(value));
        }
    }
    fields
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

fn default_lanes() -> BTreeMap<String, LaneSnapshot> {
    ["main", "review", "merge"]
        .into_iter()
        .map(|lane| (lane.to_string(), default_lane(lane)))
        .collect()
}

fn default_lane(lane: &str) -> LaneSnapshot {
    LaneSnapshot {
        lane: lane.to_string(),
        status: "idle".into(),
        action: None,
        selected: None,
        target: None,
        max_concurrent: None,
        recover: None,
        updated_at_ms: None,
        latest_line: None,
    }
}

fn record_error(
    app: &AppHandle,
    manager: &State<'_, LoopManager>,
    message: String,
) -> Result<(), String> {
    let error = AutoloopError {
        message,
        at_ms: now_ms(),
    };
    if let Ok(mut runtime) = manager.inner.lock() {
        runtime.state.running = false;
        runtime.state.stopping = false;
        runtime.state.pid = None;
        runtime.state.error = Some(error.message.clone());
        runtime.state.stopped_at_ms = Some(error.at_ms);
    }
    emit(app, "autoloop:error", &error);
    Ok(())
}

fn emit<T: Serialize + Clone>(app: &AppHandle, event: &str, payload: &T) {
    let _ = app.emit(event, payload.clone());
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn main() {
    tauri::Builder::default()
        .manage(LoopManager::default())
        .invoke_handler(tauri::generate_handler![
            start_autoloop,
            stop_autoloop,
            get_loop_state,
            get_runtime_snapshot,
            get_operator_overview,
            get_read_surface
        ])
        .run(tauri::generate_context!())
        .expect("error while running Shea Symphony App");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_autopilot_lane_line() {
        let lane = parse_autoloop_lane(
            "autopilot_loop_lane lane=review status=completed action=lane_tick_completed selected=#421 target=HumanReview max_concurrent=2 recover=false",
            42,
        )
        .unwrap();

        assert_eq!(lane.lane, "review");
        assert_eq!(lane.status, "completed");
        assert_eq!(lane.action.as_deref(), Some("lane_tick_completed"));
        assert_eq!(lane.selected.as_deref(), Some("#421"));
        assert_eq!(lane.max_concurrent, Some(2));
        assert_eq!(lane.recover, Some(false));
        assert_eq!(lane.updated_at_ms, Some(42));
    }

    #[test]
    fn parses_autopilot_running_lane_line() {
        let lane = parse_autoloop_lane(
            "autopilot_loop_lane lane=main status=running action=tick_started selected=#364 target=AgentReview max_concurrent=3 recover=true",
            84,
        )
        .unwrap();

        assert_eq!(lane.lane, "main");
        assert_eq!(lane.status, "running");
        assert_eq!(lane.action.as_deref(), Some("tick_started"));
        assert_eq!(lane.selected.as_deref(), Some("#364"));
        assert_eq!(lane.target.as_deref(), Some("AgentReview"));
    }

    #[test]
    fn parses_autopilot_json_lane_event() {
        let event = parse_autoloop_event(
            r##"{"schema_version":1,"source":"shea-symphony","event":"autopilot_loop_lane","payload":{"lane":"review","status":"running","action":"tick_started","selected_issue":{"identifier":"#364","title":"Issue title","state":"Agent Review","url":null,"priority":null,"pull_request":null},"target_state":"Human Review | Rework","max_concurrent":2,"recover":false}}"##,
        )
        .unwrap();
        let lane = parse_autoloop_lane_event(Some(&event), 84).unwrap();

        assert_eq!(lane.lane, "review");
        assert_eq!(lane.status, "running");
        assert_eq!(lane.action.as_deref(), Some("tick_started"));
        assert_eq!(lane.selected.as_deref(), Some("#364"));
        assert_eq!(lane.target.as_deref(), Some("Human Review | Rework"));
        assert_eq!(lane.max_concurrent, Some(2));
    }

    #[test]
    fn wraps_plain_autopilot_stdout_as_json_event() {
        let event = parse_autoloop_text_event(
            "stdout",
            "run_loop_action=backend issue=#364 backend=codex command='codex app-server'",
        )
        .unwrap();

        assert_eq!(event["event"], "autopilot_cli_line");
        assert_eq!(event["payload"]["kind"], "run_loop_action");
        assert_eq!(event["payload"]["fields"]["issue"], "#364");
        assert_eq!(event["payload"]["fields"]["command"], "codex app-server");
    }

    #[test]
    fn wraps_latest_status_line_as_json_event() {
        let event = parse_autoloop_text_event(
            "stdout",
            "Latest: main | #364 | running | backend | Issue title | actor=Shea Symphony Agent | next=save result",
        )
        .unwrap();

        assert_eq!(event["event"], "autopilot_cli_line");
        assert_eq!(event["payload"]["kind"], "latest");
        assert_eq!(event["payload"]["fields"]["issue"], "#364");
        assert_eq!(event["payload"]["fields"]["action"], "backend");
        assert_eq!(event["payload"]["fields"]["next"], "save result");
    }

    #[test]
    fn ignores_non_lane_lines() {
        assert!(parse_autoloop_lane("autopilot_loop=stopped reason=max_iterations", 1).is_none());
    }

    #[test]
    fn omits_none_selected_target() {
        let lane = parse_autoloop_lane(
            "autopilot_loop_lane lane=main status=completed action=lane_tick_completed selected=none target=none max_concurrent=1 recover=true",
            7,
        )
        .unwrap();

        assert_eq!(lane.selected, None);
        assert_eq!(lane.target, None);
        assert_eq!(lane.recover, Some(true));
    }

    #[test]
    fn applies_autopilot_result_line_to_snapshot() {
        let mut state = LoopStateSnapshot::default();

        assert!(apply_autoloop_result(
            &mut state,
            "autopilot_loop_result iteration=1 mode=dry-run order=main,review,merge recover=true main_max_concurrent=1 review_max_concurrent=2 merge_max_concurrent=3",
        ));

        assert_eq!(state.mode, "dry-run");
        assert_eq!(state.lanes["main"].max_concurrent, Some(1));
        assert_eq!(state.lanes["review"].max_concurrent, Some(2));
        assert_eq!(state.lanes["merge"].max_concurrent, Some(3));
    }

    #[test]
    fn applies_autopilot_json_result_to_snapshot() {
        let mut state = LoopStateSnapshot::default();
        let event = parse_autoloop_event(
            r#"{"schema_version":1,"source":"shea-symphony","event":"autopilot_loop_result","payload":{"mode":"write","settings":{"main_max_concurrent":3,"review_max_concurrent":2,"merge_max_concurrent":1}}}"#,
        )
        .unwrap();

        assert!(apply_autoloop_event(&mut state, Some(&event), 101));

        assert_eq!(state.mode, "write");
        assert_eq!(state.lanes["main"].max_concurrent, Some(3));
        assert_eq!(state.lanes["review"].max_concurrent, Some(2));
        assert_eq!(state.lanes["merge"].max_concurrent, Some(1));
    }

    #[test]
    fn applies_autopilot_stopped_line_to_snapshot() {
        let mut state = LoopStateSnapshot {
            running: true,
            stopping: true,
            pid: Some(123),
            ..LoopStateSnapshot::default()
        };

        assert!(apply_autoloop_stopped(
            &mut state,
            "autopilot_loop=stopped reason=max_iterations iterations=1",
            99,
        ));

        assert!(!state.running);
        assert!(!state.stopping);
        assert_eq!(state.pid, None);
        assert_eq!(state.stopped_at_ms, Some(99));
    }
}
