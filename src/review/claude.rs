use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::agent::{
    claude_stream_json_args, message_field, run_claude_stream_json_review,
    set_claude_resume_session, PreparedRun,
};
use crate::config::ReviewConfig;
use crate::model::AgentEvent;

use super::structured::{
    artifact_paths, artifact_texts, completed_summary, executable_path, redact_command_preview,
    review_output_schema, session_id, structured_report, terminal_error, workspace_state,
};
use super::{
    review_job_id, AgentReviewReport, ReviewBackend, ReviewBackendCommand, ReviewError, ReviewJob,
    ReviewJobState, ReviewRequest,
};

const BACKEND_NAME: &str = "claude-code";
const REVIEW_SCHEMA_PREVIEW: &str = "<Shea Review JSON schema>";

pub(super) struct ClaudeCodeReviewBackend {
    command: String,
    timeout_ms: u64,
    runs: Arc<Mutex<BTreeMap<String, ClaudeReviewRun>>>,
}

struct ClaudeReviewRun {
    result: Receiver<Result<ClaudeReviewOutcome, String>>,
    cancel: Arc<AtomicBool>,
}

struct ClaudeReviewOutcome {
    artifact_path: PathBuf,
    session_id: Option<String>,
    report: Option<AgentReviewReport>,
    error: Option<String>,
}

impl ClaudeCodeReviewBackend {
    pub(super) fn from_config(config: &ReviewConfig) -> Self {
        Self {
            command: config.claude_command.clone(),
            timeout_ms: config.timeout_ms,
            runs: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

impl ReviewBackend for ClaudeCodeReviewBackend {
    fn kind(&self) -> &'static str {
        BACKEND_NAME
    }

    fn start(&self, request: ReviewRequest) -> Result<ReviewJob, ReviewError> {
        fs::create_dir_all(&request.artifact_root)
            .map_err(|error| ReviewError::Artifact(error.to_string()))?;
        if !request.workspace.is_dir() {
            return Err(ReviewError::Backend(format!(
                "Claude Review workspace does not exist: {}",
                request.workspace.display()
            )));
        }

        let before = workspace_state(&request.workspace).map_err(ReviewError::Backend)?;
        let id = review_job_id("claude-review");
        let issue_ref = request.issue.identifier.clone();
        let prompt = claude_review_prompt(&request.prompt);
        let prompt_path = request.artifact_root.join(format!("{id}.prompt.md"));
        fs::write(&prompt_path, &prompt)
            .map_err(|error| ReviewError::Artifact(error.to_string()))?;
        let output_path = request.artifact_root.join(format!("{id}.output.json"));
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_worker = Arc::clone(&cancel);
        let command = self.command.clone();
        let timeout_ms = self.timeout_ms;
        let issue = request.issue;
        let workspace = request.workspace;
        let worker_id = id.clone();
        let prompt_path_for_worker = prompt_path.clone();
        let (result_tx, result_rx) = mpsc::channel();

        thread::spawn(move || {
            let result = execute_claude_review(ClaudeReviewExecution {
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
                ClaudeReviewRun {
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
                job.error = Some("Claude Review job runtime was not found.".into());
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
                job.backend_session_id = outcome.session_id;
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
                job.error = Some("Claude Review worker stopped without a terminal result.".into());
            }
        }
        Ok(job)
    }

    fn command_preview(&self) -> Option<ReviewBackendCommand> {
        Some(ReviewBackendCommand {
            mode: "stream-json",
            command: redact_command_preview(&self.command),
            args: claude_review_command_args(),
        })
    }

    fn prelaunch_error(&self) -> Option<String> {
        claude_prelaunch_error(&self.command)
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
                    job.backend_session_id = outcome.session_id;
                }
                Ok(Err(_)) | Err(RecvTimeoutError::Disconnected | RecvTimeoutError::Timeout) => {}
            }
        }
        if job.state == ReviewJobState::Running {
            job.state = ReviewJobState::Cancelled;
            job.error = Some("Claude Review job was cancelled before a valid report.".into());
        }
        Ok(job)
    }
}

struct ClaudeReviewExecution {
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
    timeout_ms: u64,
    before: Vec<u8>,
    cancel: Arc<AtomicBool>,
}

fn execute_claude_review(input: ClaudeReviewExecution) -> Result<ClaudeReviewOutcome, String> {
    let command = claude_review_command(&input.command)?;
    let mut prepared = PreparedRun {
        backend: BACKEND_NAME.into(),
        workspace: input.workspace.clone(),
        prompt: input.prompt,
        prompt_artifact_path: Some(input.prompt_path.clone()),
        command: Some(command),
        timeout_ms: input.timeout_ms,
        stall_timeout_ms: 0,
        model: None,
        reasoning_effort: None,
        approval_policy: None,
        sandbox: None,
        turn_sandbox_policy: None,
        app_server_resume_thread_id: None,
        profile_id: None,
        instance_name: None,
        env: BTreeMap::new(),
        actor_role: Some("independent_review_agent".into()),
        actor_label: Some("Claude Code Review".into()),
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

    let first_events = run_claude_stream_json_review(prepared.clone(), &input.cancel)
        .map_err(|error| error.to_string())?;
    let mut attempts = vec![first_events];

    // A new Review job starts without resume state. Only this in-memory job may
    // bind its own initialized Claude session to one interrupted retry.
    if interrupted_before_terminal(&attempts[0]) && !input.cancel.load(Ordering::Relaxed) {
        if let Some(recorded_session_id) = session_id(&attempts[0]) {
            set_claude_resume_session(&mut prepared, recorded_session_id);
            prepared.prompt = claude_review_resume_prompt();
            prepared.prompt_artifact_path = Some(
                input
                    .prompt_path
                    .with_file_name(format!("{}.resume.prompt.md", input.id)),
            );
            prepared.attempt = 2;
            attempts.push(
                run_claude_stream_json_review(prepared, &input.cancel)
                    .map_err(|error| error.to_string())?,
            );
        }
    }

    let after = workspace_state(&input.workspace)?;
    let workspace_unchanged = input.before == after;
    let final_events = attempts
        .last()
        .expect("Claude Review has at least one attempt");
    let structured_text = completed_summary(final_events);
    let reviewer_session_id = session_id(final_events).or_else(|| session_id(&attempts[0]));
    let stderr = artifact_texts(&attempts, "stderr_artifact=");
    let exit_status = message_field(final_events, "exit_status=");

    let report_result = if !workspace_unchanged {
        Err("Claude Review changed the read-only Review workspace; review cannot pass".into())
    } else if let Some(error) = terminal_error(final_events) {
        Err(error)
    } else {
        match structured_text.as_deref() {
            Some(text) => structured_report(
                text,
                BACKEND_NAME,
                "Claude Review",
                reviewer_session_id.clone(),
                stderr.clone(),
                exit_status.clone(),
            ),
            None => Err("Claude Review completed without a structured report".into()),
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
        "native_json_schema": true,
        "session_id": reviewer_session_id,
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

    Ok(ClaudeReviewOutcome {
        artifact_path: input.output_path,
        session_id: reviewer_session_id,
        report,
        error,
    })
}

fn claude_review_prompt(prompt: &str) -> String {
    let schema = serde_json::to_string_pretty(&review_output_schema())
        .expect("static Claude Review schema serializes");
    format!("{prompt}\n\n## Required Native JSON Schema\n\n{schema}")
}

/// Review-specific arguments shown without embedding the full schema in
/// operator-facing command previews.
pub(super) fn claude_review_command_args() -> Vec<String> {
    let mut args = vec!["--json-schema".into(), REVIEW_SCHEMA_PREVIEW.into()];
    args.extend(claude_stream_json_args());
    args
}

fn claude_review_command(command: &str) -> Result<String, String> {
    let schema = serde_json::to_string(&review_output_schema())
        .map_err(|error| format!("could not serialize Claude Review schema: {error}"))?;
    Ok(format!(
        "{} --json-schema {}",
        command.trim(),
        shell_quote(&schema)
    ))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn claude_review_resume_prompt() -> String {
    claude_review_prompt(
        "Continue only this interrupted independent Review job. Re-inspect as needed and return the required structured review report.",
    )
}

fn interrupted_before_terminal(events: &[AgentEvent]) -> bool {
    session_id(events).is_some()
        && events.iter().any(|event| {
            matches!(
                event,
                AgentEvent::Failed { error, .. }
                    if error.contains("stream ended before a terminal result event")
            )
        })
}

fn claude_prelaunch_error(command: &str) -> Option<String> {
    let configured = configured_executable(command).unwrap_or("claude");
    let path = executable_path(command);
    if path.as_deref().is_some_and(Path::is_file) {
        None
    } else {
        Some(format!(
            "Claude Code executable `{configured}` was not found; configure review_lane.claude_command or the worker PATH"
        ))
    }
}

fn configured_executable(command: &str) -> Option<&str> {
    command
        .split_whitespace()
        .map(|token| token.trim_matches(|character| matches!(character, '\'' | '"' | ';')))
        .find(|token| (token.starts_with('/') || !token.contains('=')) && *token != "env")
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::Duration;

    use crate::config::RuntimeConfig;
    use crate::model::TrackerIssue;
    use crate::workflow::WorkflowDefinition;

    use super::*;

    const FIXTURE: &str = r#"#!/bin/sh
read input || exit 2
printf '%s\n' "$*" >> "$SHEA_TEST_LOG"
session_id="claude-$$"
case "$SHEA_TEST_MODE" in
  resume) session_id=claude-resume ;;
esac
if [ "$SHEA_TEST_MODE" = startup ]; then
  exit 9
fi
printf '{"type":"system","subtype":"init","session_id":"%s"}\n' "$session_id"
case "$SHEA_TEST_MODE" in
  malformed)
    printf '{bad-json\n'
    ;;
  truncated)
    exit 0
    ;;
  unexpected)
    exit 9
    ;;
  missing-report)
    printf '%s%s%s\n' '{"type":"result","subtype":"success","is_error":false,"session_id":"' "$session_id" '","result":"completed"}'
    ;;
  error)
    printf '%s%s%s\n' '{"type":"result","subtype":"error_during_execution","is_error":true,"session_id":"' "$session_id" '","result":"fixture failure"}'
    ;;
  stall)
    sleep 2
    ;;
  resume)
    case "$*" in
      *--resume*)
        printf '%s%s%s\n' '{"type":"result","subtype":"success","is_error":false,"session_id":"' "$session_id" '","result":"{\"summary\":\"Resumed pass.\",\"terminal_classification\":\"pass\",\"findings\":[]}"}'
        ;;
      *) exit 0 ;;
    esac
    ;;
  finding)
    printf '%s%s%s\n' '{"type":"result","subtype":"success","is_error":false,"session_id":"' "$session_id" '","result":"{\"summary\":\"One defect.\",\"terminal_classification\":\"rework\",\"findings\":[{\"class\":\"confirmed\",\"severity\":\"high\",\"title\":\"Wrong result\",\"body\":\"The branch returns the wrong value.\",\"file\":\"src/lib.rs\",\"line\":7,\"evidence\":\"assert_eq failed\"}]}"}'
    ;;
  mutation)
    printf 'mutated\n' >> tracked.txt
    printf '%s%s%s\n' '{"type":"result","subtype":"success","is_error":false,"session_id":"' "$session_id" '","result":"{\"summary\":\"Pass despite mutation.\",\"terminal_classification\":\"pass\",\"findings\":[]}"}'
    ;;
  *)
    printf '%s%s%s\n' '{"type":"result","subtype":"success","is_error":false,"session_id":"' "$session_id" '","result":"{\"summary\":\"No defects.\",\"terminal_classification\":\"pass\",\"findings\":[]}"}'
    ;;
esac
"#;

    fn fixture(temp: &tempfile::TempDir) -> PathBuf {
        let executable = temp.path().join("claude");
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
            title: "Claude Review fixture".into(),
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

    fn backend(temp: &tempfile::TempDir, mode: &str, timeout_ms: u64) -> ClaudeCodeReviewBackend {
        let executable = fixture(temp);
        let log = temp.path().join(format!("{mode}.requests.log"));
        ClaudeCodeReviewBackend {
            command: format!(
                "env SHEA_TEST_MODE={mode} SHEA_TEST_LOG={} {}",
                log.display(),
                executable.display()
            ),
            timeout_ms,
            runs: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    fn request(temp: &tempfile::TempDir, workspace: &Path, identifier: &str) -> ReviewRequest {
        ReviewRequest {
            issue: issue(identifier),
            prompt: "Review only.".into(),
            workspace: workspace.to_path_buf(),
            artifact_root: temp.path().join("artifacts"),
        }
    }

    fn run(backend: &ClaudeCodeReviewBackend, request: ReviewRequest) -> ReviewJob {
        let job = backend.start(request).unwrap();
        // Keep the test watchdog outside the configured backend deadline so a
        // real provider receives the same timeout contract as production.
        let watchdog_ms = backend.timeout_ms.saturating_add(5_000);
        super::super::poll_review_job_until_terminal(
            backend,
            job,
            Duration::from_millis(watchdog_ms),
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
            request(&temp, &workspace, "#514"),
        );
        let finding = run(
            &backend(&temp, "finding", 5_000),
            request(&temp, &workspace, "#515"),
        );

        assert_eq!(pass.state, ReviewJobState::Completed, "{pass:?}");
        assert_eq!(
            pass.backend_session_id,
            pass.report.as_ref().unwrap().session_id
        );
        let ledger_path =
            super::super::write_review_job_ledger_record(temp.path(), &issue("#514"), &pass)
                .unwrap();
        let ledger: super::super::ReviewJobLedgerRecord =
            serde_json::from_slice(&fs::read(ledger_path).unwrap()).unwrap();
        assert_eq!(ledger.backend_session_id, pass.backend_session_id);
        assert!(pass.report.as_ref().unwrap().findings.is_empty());
        let finding = &finding.report.as_ref().unwrap().findings[0];
        assert_eq!(finding.class, super::super::ReviewFindingClass::Confirmed);
        assert_eq!(finding.severity.as_deref(), Some("high"));
        assert_eq!(finding.file.as_deref(), Some("src/lib.rs"));
        assert_eq!(finding.line, Some(7));
        assert_eq!(finding.evidence.as_deref(), Some("assert_eq failed"));
        assert_eq!(fs::read(workspace.join("tracked.txt")).unwrap(), before);
    }

    #[test]
    fn new_jobs_are_fresh_and_parallel_artifacts_and_sessions_are_isolated() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = workspace(&temp);
        let backend = backend(&temp, "pass", 5_000);
        let first = backend.start(request(&temp, &workspace, "#514")).unwrap();
        let second = backend.start(request(&temp, &workspace, "#514")).unwrap();
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

        assert_eq!(first.state, ReviewJobState::Completed, "{first:?}");
        assert_eq!(second.state, ReviewJobState::Completed, "{second:?}");
        assert_ne!(first.id, second.id);
        assert_ne!(first.artifact_path, second.artifact_path);
        assert_ne!(first.backend_session_id, second.backend_session_id);
        for job in [first, second] {
            let artifact: serde_json::Value =
                serde_json::from_slice(&fs::read(job.artifact_path.unwrap()).unwrap()).unwrap();
            assert_eq!(artifact["attempt_count"], 1);
            assert_eq!(artifact["resumed_same_job"], false);
            assert_eq!(
                artifact["normalized_events_artifacts"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        }
        let requests = fs::read_to_string(temp.path().join("pass.requests.log")).unwrap();
        assert_eq!(requests.matches("--input-format stream-json").count(), 2);
        assert_eq!(requests.matches("--json-schema").count(), 2);
        assert!(requests.contains("terminal_classification"));
        assert!(!requests.contains("--resume"));
    }

    #[test]
    fn interrupted_job_resumes_only_its_recorded_session() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = workspace(&temp);
        let job = run(
            &backend(&temp, "resume", 5_000),
            request(&temp, &workspace, "#514"),
        );

        assert_eq!(job.state, ReviewJobState::Completed, "{job:?}");
        assert_eq!(job.backend_session_id.as_deref(), Some("claude-resume"));
        let requests = fs::read_to_string(temp.path().join("resume.requests.log")).unwrap();
        assert_eq!(requests.lines().count(), 2);
        assert_eq!(requests.matches("--resume claude-resume").count(), 1);
        let artifact: serde_json::Value =
            serde_json::from_slice(&fs::read(job.artifact_path.unwrap()).unwrap()).unwrap();
        assert_eq!(artifact["attempt_count"], 2);
        assert_eq!(artifact["resumed_same_job"], true);
    }

    #[test]
    fn malformed_truncated_missing_error_timeout_cancel_and_mutation_fail_closed() {
        for mode in [
            "malformed",
            "truncated",
            "missing-report",
            "error",
            "startup",
            "unexpected",
            "mutation",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let workspace = workspace(&temp);
            let job = run(
                &backend(&temp, mode, 250),
                request(&temp, &workspace, "#514"),
            );
            assert_eq!(job.state, ReviewJobState::Failed, "{mode}: {job:?}");
            assert!(job.report.is_none(), "{mode}");
            assert!(job.error.as_deref().is_some_and(|error| !error.is_empty()));
        }

        let temp = tempfile::tempdir().unwrap();
        let workspace = workspace(&temp);
        let timed_out = run(
            &backend(&temp, "stall", 25),
            request(&temp, &workspace, "#514"),
        );
        assert_eq!(timed_out.state, ReviewJobState::Failed);
        assert!(timed_out.error.unwrap().contains("timed out"));

        let cancellation_backend = backend(&temp, "stall", 2_000);
        let running = cancellation_backend
            .start(request(&temp, &workspace, "#515"))
            .unwrap();
        let cancelled = cancellation_backend.cancel(running).unwrap();
        assert_eq!(cancelled.state, ReviewJobState::Cancelled);
        assert!(cancelled.artifact_path.as_deref().is_some_and(|path| {
            path.extension().and_then(|value| value.to_str()) == Some("json")
        }));
    }

    #[test]
    fn command_preview_is_sanitized_and_missing_executable_is_actionable() {
        let temp = tempfile::tempdir().unwrap();
        let missing = ClaudeCodeReviewBackend {
            command: format!("API_TOKEN=secret {}/missing/claude", temp.path().display()),
            timeout_ms: 100,
            runs: Arc::new(Mutex::new(BTreeMap::new())),
        };
        let preview = missing.command_preview().unwrap();
        assert_eq!(preview.mode, "stream-json");
        assert!(preview.command.contains("API_TOKEN=[redacted]"));
        assert_eq!(preview.args, claude_review_command_args());
        assert!(missing.prelaunch_error().unwrap().contains("was not found"));
    }

    #[test]
    #[ignore = "requires an operator-configured local read-only Claude Code command"]
    fn claude_review_live_local_read_only_uat() {
        let command = std::env::var("SHEA_CLAUDE_REVIEW_UAT_COMMAND").expect(
            "set SHEA_CLAUDE_REVIEW_UAT_COMMAND to a read-only Claude executable or wrapper",
        );
        let workflow_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(".shea/workflows/shea-symphony.md");
        let workflow = WorkflowDefinition::load(&workflow_path).unwrap();
        let config = RuntimeConfig::from_workflow(&workflow, &workflow_path).unwrap();
        config.validate().unwrap();

        for (identifier, contents, prompt, expect_finding) in [
            (
                "#514-pass",
                "The configuration fallback is documented and tested.\n",
                "Review this small clean fixture. Return pass when no concrete defect exists.",
                false,
            ),
            (
                "#514-finding",
                "fn add(left: i32, right: i32) -> i32 { left - right }\n",
                "Review this seeded add implementation. Report the subtraction defect with file evidence.",
                true,
            ),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let workspace = workspace(&temp);
            fs::write(workspace.join("tracked.txt"), contents).unwrap();
            git(&workspace, &["add", "tracked.txt"]);
            git(&workspace, &["commit", "-qm", "prepare review fixture"]);
            let before = workspace_state(&workspace).unwrap();
            let mut backend = ClaudeCodeReviewBackend::from_config(&config.review);
            backend.command = command.clone();
            let mut request = request(&temp, &workspace, identifier);
            request.prompt = prompt.into();
            let job = run(&backend, request);

            assert_eq!(job.state, ReviewJobState::Completed, "{job:?}");
            let report = job.report.unwrap();
            assert_eq!(
                report
                    .findings
                    .iter()
                    .any(|finding| finding.class == super::super::ReviewFindingClass::Confirmed),
                expect_finding,
                "{report:?}"
            );
            assert_eq!(workspace_state(&workspace).unwrap(), before);
        }
    }
}
