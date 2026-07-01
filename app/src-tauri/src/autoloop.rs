use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::autoloop_events::{
    apply_autoloop_event, apply_autoloop_result, apply_autoloop_stopped, parse_autoloop_event,
    parse_autoloop_lane, parse_autoloop_lane_event, parse_autoloop_text_event, push_recent_line,
};
use crate::autoloop_state::{
    default_lanes, AutoloopError, AutoloopLine, AutoloopOptions, AutoloopStarted, AutoloopStopped,
    LoopManager, LoopRuntime, LoopStateSnapshot,
};
use crate::cli::{command_preview, now_ms, shea_command};
use crate::target_context::{TargetContext, TargetOptions};

#[tauri::command]
pub fn get_loop_state(manager: State<'_, LoopManager>) -> Result<LoopStateSnapshot, String> {
    Ok(manager
        .inner
        .lock()
        .map_err(|error| error.to_string())?
        .state
        .clone())
}

#[tauri::command]
pub fn start_autoloop(
    app: AppHandle,
    manager: State<'_, LoopManager>,
    options: Option<AutoloopOptions>,
) -> Result<LoopStateSnapshot, String> {
    let options = options.unwrap_or(AutoloopOptions {
        workflow_path: None,
        max_iterations: Some(1),
        once: None,
        continuous: Some(true),
        write: Some(false),
        signal_format: Some("json".into()),
        poll_interval_ms: None,
        main_max_concurrent: None,
        review_max_concurrent: None,
        merge_max_concurrent: None,
    });
    let target = TargetOptions {
        workflow_path: options.workflow_path.clone(),
        ..TargetOptions::default()
    };
    let workflow_path = TargetContext::from_options(Some(&target)).workflow_path;
    let write = options.write.unwrap_or(false);
    let mode = if write { "write" } else { "dry-run" }.to_string();
    let mut args = vec![
        "autopilot".to_string(),
        "loop".to_string(),
        workflow_path.clone(),
    ];
    if options.once.unwrap_or(false) {
        args.push("--once".into());
    } else if options.continuous.unwrap_or(true) {
        args.push("--continuous".into());
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
        args.push(value.to_string());
    }
    if let Some(value) = options.review_max_concurrent {
        args.push("--review-max-concurrent".into());
        args.push(value.to_string());
    }
    if let Some(value) = options.merge_max_concurrent {
        args.push("--merge-max-concurrent".into());
        args.push(value.to_string());
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

#[tauri::command]
pub fn stop_autoloop(
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
