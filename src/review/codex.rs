use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Deserialize;

use crate::agent::{
    codex_app_server_executable, is_codex_app_server_command, message_field,
    run_codex_app_server_review, PreparedRun,
};
use crate::config::ReviewConfig;
use crate::model::AgentEvent;

use super::{
    review_job_id, AgentReviewReport, ReviewBackend, ReviewBackendCommand, ReviewError,
    ReviewFinding, ReviewFindingClass, ReviewJob, ReviewJobState, ReviewRequest,
};

const BACKEND_NAME: &str = "codex-app-server";

pub(super) struct CodexAppServerReviewBackend {
    command: String,
    approval_policy: serde_json::Value,
    thread_sandbox: String,
    turn_sandbox_policy: Option<serde_json::Value>,
    timeout_ms: u64,
    runs: Arc<Mutex<BTreeMap<String, CodexReviewRun>>>,
}

struct CodexReviewRun {
    result: Receiver<Result<CodexReviewOutcome, String>>,
    cancel: Arc<AtomicBool>,
}

struct CodexReviewOutcome {
    artifact_path: PathBuf,
    thread_id: Option<String>,
    report: Option<AgentReviewReport>,
    error: Option<String>,
}

impl CodexAppServerReviewBackend {
    pub(super) fn from_config(config: &ReviewConfig) -> Self {
        Self {
            command: config.codex_command.clone(),
            approval_policy: config.codex_approval_policy.clone(),
            thread_sandbox: config.codex_thread_sandbox.clone(),
            turn_sandbox_policy: config.codex_turn_sandbox_policy.clone(),
            timeout_ms: config.timeout_ms,
            runs: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

impl ReviewBackend for CodexAppServerReviewBackend {
    fn kind(&self) -> &'static str {
        BACKEND_NAME
    }

    fn start(&self, request: ReviewRequest) -> Result<ReviewJob, ReviewError> {
        fs::create_dir_all(&request.artifact_root)
            .map_err(|error| ReviewError::Artifact(error.to_string()))?;
        if !request.workspace.is_dir() {
            return Err(ReviewError::Backend(format!(
                "Codex Review workspace does not exist: {}",
                request.workspace.display()
            )));
        }

        let before = workspace_state(&request.workspace).map_err(ReviewError::Backend)?;
        let id = review_job_id("codex-review");
        let worker_id = id.clone();
        let issue_ref = request.issue.identifier.clone();
        let prompt_path = request.artifact_root.join(format!("{id}.prompt.md"));
        fs::write(&prompt_path, &request.prompt)
            .map_err(|error| ReviewError::Artifact(error.to_string()))?;
        let output_path = request.artifact_root.join(format!("{id}.output.json"));
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_worker = Arc::clone(&cancel);
        let command = self.command.clone();
        let approval_policy = self.approval_policy.clone();
        let thread_sandbox = self.thread_sandbox.clone();
        let turn_sandbox_policy = self.turn_sandbox_policy.clone();
        let timeout_ms = self.timeout_ms;
        let issue = request.issue;
        let workspace = request.workspace;
        let prompt = request.prompt;
        let prompt_path_for_worker = prompt_path.clone();
        let (result_tx, result_rx) = mpsc::channel();

        thread::spawn(move || {
            let result = execute_codex_review(CodexReviewExecution {
                id: worker_id,
                issue_id: issue.id,
                issue_ref: issue.identifier,
                issue_title: issue.title,
                branch_name: issue.branch_name,
                workspace,
                prompt,
                prompt_path: prompt_path_for_worker,
                output_path,
                command,
                approval_policy,
                thread_sandbox,
                turn_sandbox_policy,
                timeout_ms,
                before,
                cancel: cancel_for_worker,
            });
            let _ = result_tx.send(result);
        });

        self.runs
            .lock()
            .map_err(|error| ReviewError::Backend(error.to_string()))?
            .insert(
                id.clone(),
                CodexReviewRun {
                    result: result_rx,
                    cancel,
                },
            );

        Ok(ReviewJob {
            id,
            issue_ref,
            backend: self.kind().into(),
            state: ReviewJobState::Running,
            artifact_path: Some(prompt_path),
            ledger_path: None,
            backend_session_id: None,
            report: None,
            error: None,
        })
    }

    fn poll(&self, mut job: ReviewJob) -> Result<ReviewJob, ReviewError> {
        if job.state != ReviewJobState::Running {
            return Ok(job);
        }

        let result = {
            let runs = self
                .runs
                .lock()
                .map_err(|error| ReviewError::Backend(error.to_string()))?;
            let Some(run) = runs.get(&job.id) else {
                job.state = ReviewJobState::Failed;
                job.error = Some("Codex Review job runtime was not found.".into());
                return Ok(job);
            };
            run.result.try_recv()
        };

        match result {
            Ok(Ok(outcome)) => {
                self.runs
                    .lock()
                    .map_err(|error| ReviewError::Backend(error.to_string()))?
                    .remove(&job.id);
                job.artifact_path = Some(outcome.artifact_path);
                job.backend_session_id = outcome.thread_id;
                job.report = outcome.report;
                job.error = outcome.error;
                job.state = if job.error.is_some() {
                    ReviewJobState::Failed
                } else {
                    ReviewJobState::Completed
                };
            }
            Ok(Err(error)) => {
                self.runs
                    .lock()
                    .map_err(|lock_error| ReviewError::Backend(lock_error.to_string()))?
                    .remove(&job.id);
                job.state = ReviewJobState::Failed;
                job.error = Some(error);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.runs
                    .lock()
                    .map_err(|error| ReviewError::Backend(error.to_string()))?
                    .remove(&job.id);
                job.state = ReviewJobState::Failed;
                job.error = Some("Codex Review worker stopped without a terminal result.".into());
            }
        }
        Ok(job)
    }

    fn command_preview(&self) -> Option<ReviewBackendCommand> {
        Some(ReviewBackendCommand {
            mode: "app-server",
            command: redact_command_preview(&self.command),
            args: Vec::new(),
        })
    }

    fn prelaunch_error(&self) -> Option<String> {
        codex_prelaunch_error(&self.command)
    }

    fn cancel(&self, mut job: ReviewJob) -> Result<ReviewJob, ReviewError> {
        if let Some(run) = self
            .runs
            .lock()
            .map_err(|error| ReviewError::Backend(error.to_string()))?
            .remove(&job.id)
        {
            run.cancel.store(true, Ordering::Relaxed);
            match run
                .result
                .recv_timeout(std::time::Duration::from_millis(500))
            {
                Ok(Ok(outcome)) => {
                    job.artifact_path = Some(outcome.artifact_path);
                    job.backend_session_id = outcome.thread_id;
                }
                Ok(Err(_)) | Err(RecvTimeoutError::Disconnected | RecvTimeoutError::Timeout) => {}
            }
        }
        if job.state == ReviewJobState::Running {
            job.state = ReviewJobState::Cancelled;
            job.error = Some("Codex Review job was cancelled before a valid report.".into());
        }
        Ok(job)
    }
}

struct CodexReviewExecution {
    id: String,
    issue_id: String,
    issue_ref: String,
    issue_title: String,
    branch_name: Option<String>,
    workspace: PathBuf,
    prompt: String,
    prompt_path: PathBuf,
    output_path: PathBuf,
    command: String,
    approval_policy: serde_json::Value,
    thread_sandbox: String,
    turn_sandbox_policy: Option<serde_json::Value>,
    timeout_ms: u64,
    before: Vec<u8>,
    cancel: Arc<AtomicBool>,
}

fn execute_codex_review(input: CodexReviewExecution) -> Result<CodexReviewOutcome, String> {
    let schema = codex_review_output_schema();
    let mut prepared = PreparedRun {
        backend: BACKEND_NAME.into(),
        workspace: input.workspace.clone(),
        prompt: input.prompt,
        prompt_artifact_path: Some(input.prompt_path.clone()),
        command: Some(input.command.clone()),
        timeout_ms: input.timeout_ms,
        stall_timeout_ms: input.timeout_ms,
        model: None,
        reasoning_effort: None,
        approval_policy: Some(input.approval_policy.to_string()),
        sandbox: Some(normalize_read_only_sandbox(&input.thread_sandbox).into()),
        turn_sandbox_policy: input.turn_sandbox_policy.map(normalize_turn_sandbox),
        app_server_resume_thread_id: None,
        profile_id: None,
        instance_name: None,
        env: BTreeMap::new(),
        actor_role: Some("independent_review_agent".into()),
        actor_label: Some("Codex App-Server Review".into()),
        git_author: None,
        issue_id: Some(input.issue_id),
        issue_identifier: Some(input.issue_ref.clone()),
        issue_title: Some(input.issue_title),
        lane: Some("review".into()),
        run_id: Some(input.id.clone()),
        attempt: 1,
        branch_name: input.branch_name,
        session_registry_path: None,
    };

    let first_events = run_codex_app_server_review(prepared.clone(), &schema, &input.cancel)
        .map_err(|error| error.to_string())?;
    let mut attempts = vec![first_events];

    // Only this in-memory job may continue its recorded thread. A new Review
    // job always starts with `app_server_resume_thread_id: None` above.
    if interrupted_before_terminal(&attempts[0]) && !input.cancel.load(Ordering::Relaxed) {
        let thread_id = app_server_identity(&attempts[0], "thread_id=").ok_or_else(|| {
            "interrupted Codex Review did not record a resumable thread id".to_string()
        })?;
        prepared.app_server_resume_thread_id = Some(thread_id);
        prepared.prompt = "Continue the same interrupted Review job and return the required structured review report.".into();
        prepared.prompt_artifact_path = Some(
            input
                .prompt_path
                .with_file_name(format!("{}.resume.prompt.md", input.id)),
        );
        prepared.attempt = 2;
        attempts.push(
            run_codex_app_server_review(prepared, &schema, &input.cancel)
                .map_err(|error| error.to_string())?,
        );
    }

    let after = workspace_state(&input.workspace)?;
    let workspace_unchanged = input.before == after;
    let final_events = attempts
        .last()
        .expect("Codex Review has at least one attempt");
    let terminal_error = final_events.iter().find_map(|event| match event {
        AgentEvent::Failed { error, .. } => Some(error.clone()),
        _ => None,
    });
    let structured_text = extract_agent_output(final_events);
    let thread_id = app_server_identity(final_events, "thread_id=")
        .or_else(|| app_server_identity(&attempts[0], "thread_id="));
    let stderr = artifact_texts(&attempts, "stderr_artifact=");
    let exit_status = message_field(final_events, "exit_status=");

    let report_result = if !workspace_unchanged {
        Err("Codex Review changed the read-only Review workspace; review cannot pass".into())
    } else if let Some(error) = terminal_error {
        Err(error)
    } else {
        match structured_text.as_deref() {
            Some(text) => {
                structured_report(text, thread_id.clone(), stderr.clone(), exit_status.clone())
            }
            None => Err("Codex Review completed without a structured report".into()),
        }
    };

    let (report, error) = match report_result {
        Ok(report) => (Some(report), None),
        Err(error) => (None, Some(error)),
    };
    let artifact = serde_json::json!({
        "job_id": input.id,
        "issue_ref": input.issue_ref,
        "backend": BACKEND_NAME,
        "command_preview": redact_command_preview(&input.command),
        "thread_id": thread_id,
        "workspace": input.workspace.display().to_string(),
        "workspace_unchanged": workspace_unchanged,
        "attempt_count": attempts.len(),
        "resumed_same_job": attempts.len() > 1,
        "protocol_artifacts": artifact_paths(&attempts, "protocol_artifact="),
        "stderr_artifacts": artifact_paths(&attempts, "stderr_artifact="),
        "normalized_events_artifacts": artifact_paths(&attempts, "normalized_events_artifact="),
        "exit_status": exit_status,
        "structured_output": structured_text,
        "error": error.clone(),
    });
    fs::write(
        &input.output_path,
        serde_json::to_string_pretty(&artifact).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    Ok(CodexReviewOutcome {
        artifact_path: input.output_path,
        thread_id,
        report,
        error,
    })
}

fn codex_review_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "terminal_classification": {
                "type": "string",
                "enum": ["pass", "rework", "needs_context"]
            },
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "class": {
                            "type": "string",
                            "enum": ["confirmed", "plausible", "rejected", "needs_context"]
                        },
                        "severity": {
                            "type": "string",
                            "enum": ["critical", "high", "medium", "low", "note"]
                        },
                        "title": { "type": "string" },
                        "body": { "type": "string" },
                        "file": { "type": ["string", "null"] },
                        "line": { "type": ["integer", "null"], "minimum": 1 },
                        "evidence": { "type": "string" }
                    },
                    "required": ["class", "severity", "title", "body", "file", "line", "evidence"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["summary", "terminal_classification", "findings"],
        "additionalProperties": false
    })
}

#[derive(Deserialize)]
struct StructuredReview {
    summary: String,
    terminal_classification: StructuredTerminal,
    findings: Vec<StructuredFinding>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StructuredTerminal {
    Pass,
    Rework,
    NeedsContext,
}

#[derive(Deserialize)]
struct StructuredFinding {
    class: StructuredFindingClass,
    severity: String,
    title: String,
    body: String,
    file: Option<String>,
    line: Option<u64>,
    evidence: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StructuredFindingClass {
    Confirmed,
    Plausible,
    Rejected,
    NeedsContext,
}

fn structured_report(
    raw: &str,
    thread_id: Option<String>,
    stderr: String,
    exit_status: Option<String>,
) -> Result<AgentReviewReport, String> {
    let parsed: StructuredReview = serde_json::from_str(raw.trim())
        .map_err(|error| format!("Codex Review returned malformed structured output: {error}"))?;
    if parsed.summary.trim().is_empty() {
        return Err("Codex Review structured output has an empty summary".into());
    }

    let mut findings = Vec::with_capacity(parsed.findings.len());
    for finding in parsed.findings {
        if finding.title.trim().is_empty()
            || finding.body.trim().is_empty()
            || finding.evidence.trim().is_empty()
        {
            return Err(
                "Codex Review structured finding is missing title, body, or evidence".into(),
            );
        }
        if !matches!(
            finding.severity.as_str(),
            "critical" | "high" | "medium" | "low" | "note"
        ) {
            return Err(format!(
                "Codex Review structured finding has unsupported severity `{}`",
                finding.severity
            ));
        }
        if finding
            .file
            .as_deref()
            .is_some_and(|file| file.trim().is_empty())
        {
            return Err("Codex Review structured finding has an empty file".into());
        }
        if finding.line == Some(0) {
            return Err("Codex Review structured finding line must be one-based".into());
        }
        if finding.line.is_some()
            && finding
                .file
                .as_deref()
                .is_none_or(|file| file.trim().is_empty())
        {
            return Err("Codex Review structured finding has a line without a file".into());
        }
        let class = match finding.class {
            StructuredFindingClass::Confirmed => ReviewFindingClass::Confirmed,
            StructuredFindingClass::Plausible => ReviewFindingClass::Plausible,
            StructuredFindingClass::Rejected => ReviewFindingClass::Rejected,
            StructuredFindingClass::NeedsContext => ReviewFindingClass::NeedsContext,
        };
        findings.push(ReviewFinding {
            class,
            title: finding.title,
            body: finding.body,
            severity: Some(finding.severity),
            file: finding.file,
            line: finding.line,
            evidence: Some(finding.evidence),
        });
    }

    let has_confirmed = findings
        .iter()
        .any(|finding| finding.class == ReviewFindingClass::Confirmed);
    let has_needs_context = findings
        .iter()
        .any(|finding| finding.class == ReviewFindingClass::NeedsContext);
    match parsed.terminal_classification {
        StructuredTerminal::Pass if has_confirmed || has_needs_context => {
            return Err("Codex Review pass classification conflicts with blocking findings".into())
        }
        StructuredTerminal::Rework if !has_confirmed => {
            return Err("Codex Review rework classification has no confirmed finding".into())
        }
        StructuredTerminal::NeedsContext if !has_needs_context => {
            return Err(
                "Codex Review needs_context classification has no Needs Context finding".into(),
            )
        }
        StructuredTerminal::NeedsContext if has_confirmed => {
            return Err(
                "Codex Review needs_context classification conflicts with a confirmed finding"
                    .into(),
            )
        }
        _ => {}
    }

    Ok(AgentReviewReport {
        reviewer_backend: BACKEND_NAME.into(),
        findings,
        summary: Some(parsed.summary),
        stdout: Some(raw.to_string()),
        stderr: Some(stderr),
        exit_status,
        session_id: thread_id,
    })
}

fn extract_agent_output(events: &[AgentEvent]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        let AgentEvent::Message { text, .. } = event else {
            return None;
        };
        let payload = serde_json::from_str::<serde_json::Value>(text).ok()?;
        if payload.get("method").and_then(serde_json::Value::as_str) != Some("item/completed") {
            return None;
        }
        let item = payload.pointer("/params/item")?;
        match item.get("type").and_then(serde_json::Value::as_str) {
            Some("agentMessage") => item.get("text").and_then(serde_json::Value::as_str),
            Some("exitedReviewMode") => item.get("review").and_then(serde_json::Value::as_str),
            _ => None,
        }
        .map(str::to_string)
    })
}

fn interrupted_before_terminal(events: &[AgentEvent]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            AgentEvent::Failed { error, .. }
                if error.contains("exited before a terminal turn event")
                    || error.contains("exited unexpectedly with status")
        )
    })
}

fn app_server_identity(events: &[AgentEvent], field: &str) -> Option<String> {
    message_field(events, field)
}

fn artifact_paths(attempts: &[Vec<AgentEvent>], field: &str) -> Vec<String> {
    attempts
        .iter()
        .filter_map(|events| message_field(events, field))
        .collect()
}

fn artifact_texts(attempts: &[Vec<AgentEvent>], field: &str) -> String {
    artifact_paths(attempts, field)
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn workspace_state(workspace: &Path) -> Result<Vec<u8>, String> {
    let tracked = Command::new("git")
        .args(["diff", "--binary", "HEAD", "--", "."])
        .current_dir(workspace)
        .output()
        .map_err(|error| format!("could not snapshot Review workspace: {error}"))?;
    if !tracked.status.success() {
        return Err(format!(
            "could not snapshot Review workspace: {}",
            String::from_utf8_lossy(&tracked.stderr).trim()
        ));
    }
    let untracked = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .current_dir(workspace)
        .output()
        .map_err(|error| format!("could not inventory Review workspace: {error}"))?;
    if !untracked.status.success() {
        return Err(format!(
            "could not inventory Review workspace: {}",
            String::from_utf8_lossy(&untracked.stderr).trim()
        ));
    }

    let mut snapshot = tracked.stdout;
    for path in untracked
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = Path::new(std::str::from_utf8(path).map_err(|error| {
            format!("Review workspace contains a non-UTF-8 untracked path: {error}")
        })?);
        let contents = fs::read(workspace.join(relative)).map_err(|error| {
            format!(
                "could not snapshot untracked Review file {}: {error}",
                relative.display()
            )
        })?;
        snapshot.extend_from_slice(&(path.len() as u64).to_le_bytes());
        snapshot.extend_from_slice(path);
        snapshot.extend_from_slice(&(contents.len() as u64).to_le_bytes());
        snapshot.extend_from_slice(&contents);
    }
    Ok(snapshot)
}

fn normalize_read_only_sandbox(value: &str) -> &str {
    match value.trim() {
        "read-only" | "readOnly" => "readOnly",
        _ => "readOnly",
    }
}

fn normalize_turn_sandbox(mut value: serde_json::Value) -> serde_json::Value {
    if value.get("type").and_then(serde_json::Value::as_str) == Some("read-only") {
        value["type"] = serde_json::Value::String("readOnly".into());
    }
    value
}

fn codex_prelaunch_error(command: &str) -> Option<String> {
    if !is_codex_app_server_command(command) {
        return Some(
            "review_lane.codex_command must launch the Codex app-server subcommand".into(),
        );
    }
    let executable = codex_app_server_executable(command)?;
    let path = if Path::new(&executable).is_absolute() || executable.contains('/') {
        PathBuf::from(&executable)
    } else {
        std::env::var_os("PATH")
            .and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join(&executable))
                    .find(|candidate| candidate.is_file())
            })
            .unwrap_or_default()
    };
    if !path.is_file() {
        return Some(format!(
            "Codex app-server executable `{executable}` was not found; configure review_lane.codex_command or the worker PATH"
        ));
    }
    None
}

fn redact_command_preview(command: &str) -> String {
    let mut redact_next = false;
    command
        .split_whitespace()
        .map(|token| {
            if redact_next {
                redact_next = false;
                return "[redacted]".to_string();
            }
            let normalized = token.to_ascii_lowercase();
            if ["--token", "--api-key", "--secret", "--password"].contains(&normalized.as_str()) {
                redact_next = true;
                return token.to_string();
            }
            if let Some((name, _)) = token.split_once('=') {
                let name = name.to_ascii_lowercase();
                if ["token", "key", "secret", "password"]
                    .iter()
                    .any(|needle| name.contains(needle))
                {
                    return format!("{}=[redacted]", token.split_once('=').unwrap().0);
                }
            }
            token.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use crate::model::TrackerIssue;

    use super::*;

    const FIXTURE: &str = r#"#!/bin/sh
read initialize || exit 2
printf '{"jsonrpc":"2.0","id":1,"result":{}}\n'
read initialized || exit 2
read thread_request || exit 2
printf '%s\n' "$thread_request" >> "$SHEA_TEST_LOG"
case "$thread_request" in
  *'"method":"thread/resume"'*) thread_id=thread-resume ;;
  *)
    if [ "$SHEA_TEST_MODE" = resume ]; then
      thread_id=thread-resume
    else
      thread_id=thread-$$
    fi
    ;;
esac
printf '{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"%s"}}}\n' "$thread_id"
read turn_request || exit 2
printf '%s\n' "$turn_request" >> "$SHEA_TEST_LOG"
printf '{"jsonrpc":"2.0","id":3,"result":{"turn":{"id":"turn-1"}}}\n'
case "$SHEA_TEST_MODE" in
  approval)
    printf '{"jsonrpc":"2.0","id":99,"method":"item/commandExecution/requestApproval","params":{}}\n'
    ;;
  user-input)
    printf '{"jsonrpc":"2.0","id":99,"method":"item/tool/requestUserInput","params":{}}\n'
    ;;
  unknown-tool)
    printf '{"jsonrpc":"2.0","id":99,"method":"item/tool/brandNewTool","params":{}}\n'
    ;;
  malformed)
    printf '{bad-json\n'
    ;;
  truncated)
    exit 0
    ;;
  missing-report)
    printf '{"method":"turn/completed","params":{"turn":{"status":"completed"}}}\n'
    ;;
  bad-report)
    printf '%s\n' '{"method":"item/completed","params":{"item":{"type":"agentMessage","text":"not-json"}}}'
    printf '%s\n' '{"method":"turn/completed","params":{"turn":{"status":"completed"}}}'
    ;;
  stall)
    sleep 2
    ;;
  resume)
    if [ ! -e "$SHEA_TEST_MARKER" ]; then
      : > "$SHEA_TEST_MARKER"
      exit 0
    fi
    printf '%s\n' '{"method":"item/completed","params":{"item":{"type":"agentMessage","text":"{\"summary\":\"Resumed pass.\",\"terminal_classification\":\"pass\",\"findings\":[]}"}}}'
    printf '%s\n' '{"method":"turn/completed","params":{"turn":{"status":"completed"}}}'
    ;;
  finding)
    printf '%s\n' '{"method":"item/completed","params":{"item":{"type":"agentMessage","text":"{\"summary\":\"One defect.\",\"terminal_classification\":\"rework\",\"findings\":[{\"class\":\"confirmed\",\"severity\":\"high\",\"title\":\"Wrong result\",\"body\":\"The branch returns the wrong value.\",\"file\":\"src/lib.rs\",\"line\":7,\"evidence\":\"assert_eq failed\"}]}"}}}'
    printf '%s\n' '{"method":"turn/completed","params":{"turn":{"status":"completed"}}}'
    ;;
  mutation)
    printf 'mutated\n' >> tracked.txt
    printf '%s\n' '{"method":"item/completed","params":{"item":{"type":"agentMessage","text":"{\"summary\":\"Pass despite mutation.\",\"terminal_classification\":\"pass\",\"findings\":[]}"}}}'
    printf '%s\n' '{"method":"turn/completed","params":{"turn":{"status":"completed"}}}'
    ;;
  *)
    printf '%s\n' '{"method":"item/completed","params":{"item":{"type":"agentMessage","text":"{\"summary\":\"No defects.\",\"terminal_classification\":\"pass\",\"findings\":[]}"}}}'
    printf '%s\n' '{"method":"turn/completed","params":{"turn":{"status":"completed"}}}'
    ;;
esac
"#;

    fn fixture(temp: &tempfile::TempDir) -> PathBuf {
        let executable = temp.path().join("codex");
        fs::write(&executable, FIXTURE).unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        executable
    }

    fn git(workspace: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(workspace)
            .env("GIT_AUTHOR_NAME", "Shea Test")
            .env("GIT_AUTHOR_EMAIL", "shea@example.invalid")
            .env("GIT_COMMITTER_NAME", "Shea Test")
            .env("GIT_COMMITTER_EMAIL", "shea@example.invalid")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn workspace(temp: &tempfile::TempDir) -> PathBuf {
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        git(&workspace, &["init", "-q"]);
        fs::write(workspace.join("tracked.txt"), "original\n").unwrap();
        git(&workspace, &["add", "tracked.txt"]);
        git(&workspace, &["commit", "-qm", "fixture"]);
        workspace
    }

    fn issue(identifier: &str) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: identifier.trim_start_matches('#').into(),
            item_id: None,
            identifier: identifier.into(),
            title: "Codex Review fixture".into(),
            description: Some("fixture".into()),
            url: None,
            state: "Agent Review".into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: Some(format!("feature/{identifier}")),
            linked_pull_requests: Vec::new(),
            blocked_by: Vec::new(),
            project_fields: BTreeMap::new(),
            created_at: None,
            updated_at: None,
        }
    }

    fn backend(
        temp: &tempfile::TempDir,
        mode: &str,
        timeout_ms: u64,
    ) -> CodexAppServerReviewBackend {
        let executable = fixture(temp);
        let log = temp.path().join(format!("{mode}.requests.jsonl"));
        let marker = temp.path().join(format!("{mode}.resume-marker"));
        CodexAppServerReviewBackend {
            command: format!(
                "SHEA_TEST_MODE={mode} SHEA_TEST_LOG={} SHEA_TEST_MARKER={} {} app-server",
                log.display(),
                marker.display(),
                executable.display()
            ),
            approval_policy: serde_json::json!("never"),
            thread_sandbox: "read-only".into(),
            turn_sandbox_policy: Some(serde_json::json!({"type": "readOnly"})),
            timeout_ms,
            runs: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    fn request(temp: &tempfile::TempDir, workspace: &Path, identifier: &str) -> ReviewRequest {
        ReviewRequest {
            issue: issue(identifier),
            prompt: "Review only; return the required schema.".into(),
            workspace: workspace.to_path_buf(),
            artifact_root: temp.path().join("artifacts"),
        }
    }

    fn run(backend: &CodexAppServerReviewBackend, request: ReviewRequest) -> ReviewJob {
        let job = backend.start(request).unwrap();
        super::super::poll_review_job_until_terminal(
            backend,
            job,
            Duration::from_secs(10),
            Duration::from_millis(5),
        )
        .unwrap()
    }

    #[test]
    fn pass_and_confirmed_finding_preserve_structured_evidence_and_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = workspace(&temp);
        let before = fs::read(workspace.join("tracked.txt")).unwrap();

        let pass = run(
            &backend(&temp, "pass", 5_000),
            request(&temp, &workspace, "#511"),
        );
        let finding = run(
            &backend(&temp, "finding", 5_000),
            request(&temp, &workspace, "#512"),
        );

        assert_eq!(pass.state, ReviewJobState::Completed);
        assert_eq!(
            pass.backend_session_id,
            pass.report.as_ref().unwrap().session_id
        );
        let ledger = super::super::review_job_ledger_record(
            &issue("#511"),
            &pass,
            temp.path().join("pass-ledger.json"),
        );
        assert_eq!(ledger.backend_session_id, pass.backend_session_id);
        assert!(pass.report.as_ref().unwrap().findings.is_empty());
        assert_eq!(finding.state, ReviewJobState::Completed);
        let finding = &finding.report.as_ref().unwrap().findings[0];
        assert_eq!(finding.class, ReviewFindingClass::Confirmed);
        assert_eq!(finding.severity.as_deref(), Some("high"));
        assert_eq!(finding.file.as_deref(), Some("src/lib.rs"));
        assert_eq!(finding.line, Some(7));
        assert_eq!(finding.evidence.as_deref(), Some("assert_eq failed"));
        assert_eq!(fs::read(workspace.join("tracked.txt")).unwrap(), before);
    }

    #[test]
    fn new_jobs_are_fresh_and_parallel_artifacts_and_threads_are_isolated() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = workspace(&temp);
        let backend = backend(&temp, "pass", 5_000);
        let first = backend.start(request(&temp, &workspace, "#511")).unwrap();
        let second = backend.start(request(&temp, &workspace, "#511")).unwrap();
        let first = super::super::poll_review_job_until_terminal(
            &backend,
            first,
            Duration::from_secs(10),
            Duration::from_millis(5),
        )
        .unwrap();
        let second = super::super::poll_review_job_until_terminal(
            &backend,
            second,
            Duration::from_secs(10),
            Duration::from_millis(5),
        )
        .unwrap();

        assert_ne!(first.id, second.id);
        assert_ne!(first.artifact_path, second.artifact_path);
        assert_ne!(
            first.report.unwrap().session_id,
            second.report.unwrap().session_id
        );
        let requests = fs::read_to_string(temp.path().join("pass.requests.jsonl")).unwrap();
        assert_eq!(requests.matches("thread/start").count(), 2);
        assert!(!requests.contains("thread/resume"));
        assert!(requests.contains("\"approvalPolicy\":\"never\""));
        assert!(requests.contains("\"sandbox\":\"readOnly\""));
        assert!(requests.contains("\"outputSchema\""));
    }

    #[test]
    fn interrupted_job_resumes_only_its_recorded_thread() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = workspace(&temp);
        let job = run(
            &backend(&temp, "resume", 5_000),
            request(&temp, &workspace, "#511"),
        );

        assert_eq!(job.state, ReviewJobState::Completed);
        assert_eq!(
            job.report.as_ref().unwrap().session_id.as_deref(),
            Some("thread-resume")
        );
        let requests = fs::read_to_string(temp.path().join("resume.requests.jsonl")).unwrap();
        assert_eq!(requests.matches("thread/start").count(), 1);
        assert_eq!(requests.matches("thread/resume").count(), 1);
        assert!(requests.contains("\"threadId\":\"thread-resume\""));
        let artifact: serde_json::Value =
            serde_json::from_slice(&fs::read(job.artifact_path.unwrap()).unwrap()).unwrap();
        assert_eq!(artifact["attempt_count"], 2);
        assert_eq!(artifact["resumed_same_job"], true);
        assert_eq!(artifact["protocol_artifacts"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn interactive_unknown_malformed_truncated_and_missing_results_fail_closed() {
        for mode in [
            "approval",
            "user-input",
            "unknown-tool",
            "malformed",
            "truncated",
            "missing-report",
            "bad-report",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let workspace = workspace(&temp);
            let job = run(
                &backend(&temp, mode, 250),
                request(&temp, &workspace, "#511"),
            );

            assert_eq!(job.state, ReviewJobState::Failed, "{mode}: {job:?}");
            assert!(job.report.is_none(), "{mode}");
            assert!(
                job.error.as_deref().is_some_and(|error| !error.is_empty()),
                "{mode}"
            );
        }
    }

    #[test]
    fn invalid_structured_finding_metadata_and_classification_fail_closed() {
        for report in [
            serde_json::json!({
                "summary": "bad severity",
                "terminal_classification": "rework",
                "findings": [{
                    "class": "confirmed", "severity": "urgent", "title": "bug",
                    "body": "body", "file": "src/lib.rs", "line": 1, "evidence": "evidence"
                }]
            }),
            serde_json::json!({
                "summary": "bad line",
                "terminal_classification": "rework",
                "findings": [{
                    "class": "confirmed", "severity": "high", "title": "bug",
                    "body": "body", "file": "src/lib.rs", "line": 0, "evidence": "evidence"
                }]
            }),
            serde_json::json!({
                "summary": "conflicting pass",
                "terminal_classification": "pass",
                "findings": [{
                    "class": "confirmed", "severity": "high", "title": "bug",
                    "body": "body", "file": "src/lib.rs", "line": 1, "evidence": "evidence"
                }]
            }),
        ] {
            assert!(structured_report(&report.to_string(), None, String::new(), None).is_err());
        }
    }

    #[test]
    fn timeout_cancellation_startup_and_workspace_mutation_cannot_pass() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = workspace(&temp);
        let timed_out = run(
            &backend(&temp, "stall", 25),
            request(&temp, &workspace, "#511"),
        );
        assert_eq!(timed_out.state, ReviewJobState::Failed);
        assert!(timed_out.error.unwrap().contains("timed out"));

        let cancellation_backend = backend(&temp, "stall", 2_000);
        let running = cancellation_backend
            .start(request(&temp, &workspace, "#512"))
            .unwrap();
        let cancelled = cancellation_backend.cancel(running).unwrap();
        assert_eq!(cancelled.state, ReviewJobState::Cancelled);
        assert!(cancelled
            .artifact_path
            .as_deref()
            .is_some_and(|path| path.extension().and_then(|value| value.to_str()) == Some("json")));

        let mutation = run(
            &backend(&temp, "mutation", 5_000),
            request(&temp, &workspace, "#513"),
        );
        assert_eq!(mutation.state, ReviewJobState::Failed);
        assert!(mutation
            .error
            .unwrap()
            .contains("changed the read-only Review workspace"));

        let missing = CodexAppServerReviewBackend {
            command: format!("{}/missing/codex app-server", temp.path().display()),
            approval_policy: serde_json::json!("never"),
            thread_sandbox: "read-only".into(),
            turn_sandbox_policy: None,
            timeout_ms: 100,
            runs: Arc::new(Mutex::new(BTreeMap::new())),
        };
        assert!(missing.prelaunch_error().unwrap().contains("was not found"));
    }
}
