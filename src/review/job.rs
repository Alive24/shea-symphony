//! Review job lifecycle, backend contract, and durable ledger records.
#![deny(missing_docs)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::agent::{classify_usage_limit_text, UsageLimitPause};
use crate::model::TrackerIssue;
use crate::workspace::safe_identifier;

use super::{
    gemini_review_health_diagnostic, review_gate_decision_for_issue, review_worker_key,
    AgentReviewReport, GeminiReviewHealthDiagnostic, ReviewOutcome,
};

/// Lifecycle states shared by every Review backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewJobState {
    /// Accepted by a backend but not yet executing.
    Queued,
    /// Backend execution is active.
    Running,
    /// Backend execution produced a valid report.
    Completed,
    /// Backend execution ended without a valid report.
    Failed,
    /// The Review watchdog exhausted its configured deadline.
    TimedOut,
    /// The Review job was explicitly cancelled.
    Cancelled,
}

/// Provider-neutral state and evidence for one Review backend invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewJob {
    /// Unique Review job identifier.
    pub id: String,
    /// Tracker issue identifier.
    pub issue_ref: String,
    /// Selected Review backend identifier.
    pub backend: String,
    /// Current job lifecycle state.
    pub state: ReviewJobState,
    /// Primary backend evidence artifact.
    pub artifact_path: Option<PathBuf>,
    /// Durable Review job ledger path after reconciliation.
    #[serde(default)]
    pub ledger_path: Option<PathBuf>,
    /// Provider thread or session identity, including for a failed started job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_session_id: Option<String>,
    /// Validated backend-neutral report, when one was produced.
    pub report: Option<AgentReviewReport>,
    /// Actionable failure detail for non-successful jobs.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Durable backend-neutral record written after a Review job reaches a terminal state.
pub struct ReviewJobLedgerRecord {
    /// Tracker issue identifier.
    pub issue_ref: String,
    /// Tracker issue title captured with the job.
    pub issue_title: String,
    /// Unique Review job identifier.
    pub job_id: String,
    /// Existing scheduler worker key for the issue and backend.
    pub worker_key: String,
    /// Review backend identifier.
    pub backend: String,
    /// Terminal or observed Review job state.
    pub state: ReviewJobState,
    /// Backend process identifier when the backend exposes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Job start timestamp in Unix milliseconds when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    /// Last job update timestamp in Unix milliseconds when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<u64>,
    /// Primary backend artifact containing pointers to detailed evidence.
    pub artifact_path: Option<PathBuf>,
    /// Path of this ledger record.
    pub ledger_path: PathBuf,
    /// Provider thread or session identity recorded by the backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_session_id: Option<String>,
    /// Backend-neutral routing outcome.
    pub decision_outcome: ReviewOutcome,
    /// Normalized Project target state selected by existing Review routing.
    pub decision_target_state: Option<String>,
    /// Concise Review summary.
    pub summary: Option<String>,
    /// Backend failure detail, when present.
    pub error: Option<String>,
    /// Number of normalized findings.
    pub finding_count: usize,
    /// Legacy Gemini/agy health classification, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_health: Option<GeminiReviewHealthDiagnostic>,
}

/// Errors returned at the Review backend and artifact boundaries.
#[derive(Debug, Error)]
pub enum ReviewError {
    /// Backend setup or execution failure.
    #[error("review backend failed: {0}")]
    Backend(String),
    /// Evidence artifact persistence failure.
    #[error("review artifact failed: {0}")]
    Artifact(String),
}

/// Immutable input boundary passed from Review orchestration to a backend.
pub struct ReviewRequest {
    /// Normalized tracker issue being reviewed.
    pub issue: TrackerIssue,
    /// Fully rendered independent Review prompt.
    pub prompt: String,
    /// Existing isolated Review workspace, treated as read-only.
    pub workspace: PathBuf,
    /// Directory in which the backend records Review evidence.
    pub artifact_root: PathBuf,
}

impl ReviewJob {
    /// Constructs a terminal failed job when backend prelaunch is unavailable.
    pub fn failed_unavailable(
        issue_ref: impl Into<String>,
        backend: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            id: review_job_id("review-unavailable"),
            issue_ref: issue_ref.into(),
            backend: backend.into(),
            state: ReviewJobState::Failed,
            artifact_path: None,
            ledger_path: None,
            backend_session_id: None,
            report: None,
            error: Some(error.into()),
        }
    }
}

/// Provider adapter consumed by the existing Review scheduler and routing loop.
pub trait ReviewBackend {
    /// Stable backend identifier recorded in jobs and ledgers.
    fn kind(&self) -> &'static str;
    /// Starts one independent Review job.
    fn start(&self, request: ReviewRequest) -> Result<ReviewJob, ReviewError>;
    /// Polls a job without changing scheduler ownership or routing semantics.
    fn poll(&self, job: ReviewJob) -> Result<ReviewJob, ReviewError>;
    /// Returns a sanitized launch preview for operator evidence.
    fn command_preview(&self) -> Option<ReviewBackendCommand> {
        None
    }
    /// Returns an actionable launch diagnostic without starting a job.
    fn prelaunch_error(&self) -> Option<String> {
        None
    }
    /// Cancels an active job and returns its terminal local state.
    fn cancel(&self, job: ReviewJob) -> Result<ReviewJob, ReviewError> {
        Ok(job)
    }
}

/// Sanitized backend launch description recorded before execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewBackendCommand {
    /// Launch protocol, such as `print` or `app-server`.
    pub mode: &'static str,
    /// Sanitized configured executable command.
    pub command: String,
    /// Sanitized arguments supplied separately by the backend.
    pub args: Vec<String>,
}

/// Returns whether the job has reached a terminal lifecycle state.
pub fn review_job_is_terminal(job: &ReviewJob) -> bool {
    matches!(
        job.state,
        ReviewJobState::Completed
            | ReviewJobState::Failed
            | ReviewJobState::TimedOut
            | ReviewJobState::Cancelled
    )
}

/// Polls a backend until its job is terminal or the outer deadline expires.
pub fn poll_review_job_until_terminal(
    backend: &dyn ReviewBackend,
    mut job: ReviewJob,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<ReviewJob, ReviewError> {
    let started = Instant::now();

    loop {
        job = backend.poll(job)?;
        if review_job_is_terminal(&job) {
            return Ok(job);
        }

        if started.elapsed() >= timeout {
            job = backend.cancel(job)?;
            job.state = ReviewJobState::TimedOut;
            job.error = Some(format!(
                "Review backend timed out after {}ms.",
                timeout.as_millis()
            ));
            return Ok(job);
        }

        if poll_interval.is_zero() {
            thread::yield_now();
        } else {
            thread::sleep(poll_interval);
        }
    }
}

/// Persists one terminal Review job ledger record and returns its path.
pub fn write_review_job_ledger_record(
    logs_root: &Path,
    issue: &TrackerIssue,
    job: &ReviewJob,
) -> Result<PathBuf, ReviewError> {
    let job_root = logs_root.join("reviews").join("jobs");
    fs::create_dir_all(&job_root).map_err(|error| ReviewError::Artifact(error.to_string()))?;
    let path = job_root.join(format!(
        "{}.json",
        safe_identifier(&format!("{}-{}", issue.identifier, job.id))
    ));
    let record = review_job_ledger_record(issue, job, path.clone());
    let body = serde_json::to_string_pretty(&record)
        .map_err(|error| ReviewError::Artifact(error.to_string()))?;
    fs::write(&path, body).map_err(|error| ReviewError::Artifact(error.to_string()))?;
    Ok(path)
}

/// Persists a terminal Review job ledger and attaches its path to the job used
/// by downstream evidence and routing surfaces.
pub fn persist_review_job_ledger_record(
    logs_root: &Path,
    issue: &TrackerIssue,
    job: &mut ReviewJob,
) -> Result<PathBuf, ReviewError> {
    let path = write_review_job_ledger_record(logs_root, issue, job)?;
    job.ledger_path = Some(path.clone());
    Ok(path)
}

/// Constructs the durable ledger record consumed by Review status surfaces.
pub fn review_job_ledger_record(
    issue: &TrackerIssue,
    job: &ReviewJob,
    ledger_path: PathBuf,
) -> ReviewJobLedgerRecord {
    let decision = review_gate_decision_for_issue(job, issue);
    ReviewJobLedgerRecord {
        issue_ref: issue.identifier.clone(),
        issue_title: issue.title.clone(),
        job_id: job.id.clone(),
        worker_key: review_worker_key(issue, &job.backend),
        backend: job.backend.clone(),
        state: job.state.clone(),
        pid: None,
        started_at_ms: None,
        updated_at_ms: None,
        artifact_path: job.artifact_path.clone(),
        ledger_path,
        backend_session_id: job.backend_session_id.clone().or_else(|| {
            job.report
                .as_ref()
                .and_then(|report| report.session_id.clone())
        }),
        decision_outcome: decision.outcome,
        decision_target_state: decision.target_state.map(str::to_string),
        summary: job
            .report
            .as_ref()
            .and_then(|report| report.summary.clone()),
        error: job.error.clone(),
        finding_count: job
            .report
            .as_ref()
            .map(|report| report.findings.len())
            .unwrap_or_default(),
        gemini_health: gemini_review_health_diagnostic(job),
    }
}

/// Extracts a normalized usage-limit pause from backend failure evidence.
pub fn review_usage_limit_pause(job: &ReviewJob) -> Option<UsageLimitPause> {
    job.error
        .as_deref()
        .and_then(classify_usage_limit_text)
        .or_else(|| {
            job.report.as_ref().and_then(|report| {
                report
                    .stderr
                    .as_deref()
                    .and_then(classify_usage_limit_text)
                    .or_else(|| report.stdout.as_deref().and_then(classify_usage_limit_text))
                    .or_else(|| {
                        report
                            .summary
                            .as_deref()
                            .and_then(classify_usage_limit_text)
                    })
            })
        })
}

static REVIEW_JOB_ID_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn review_job_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = REVIEW_JOB_ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{millis}-{sequence}")
}
