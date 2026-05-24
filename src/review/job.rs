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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewJobState {
    Queued,
    Running,
    Completed,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewJob {
    pub id: String,
    pub issue_ref: String,
    pub backend: String,
    pub state: ReviewJobState,
    pub artifact_path: Option<PathBuf>,
    #[serde(default)]
    pub ledger_path: Option<PathBuf>,
    pub report: Option<AgentReviewReport>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewJobLedgerRecord {
    pub issue_ref: String,
    pub issue_title: String,
    pub job_id: String,
    pub worker_key: String,
    pub backend: String,
    pub state: ReviewJobState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<u64>,
    pub artifact_path: Option<PathBuf>,
    pub ledger_path: PathBuf,
    pub decision_outcome: ReviewOutcome,
    pub decision_target_state: Option<String>,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub finding_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_health: Option<GeminiReviewHealthDiagnostic>,
}

#[derive(Debug, Error)]
pub enum ReviewError {
    #[error("review backend failed: {0}")]
    Backend(String),
    #[error("review artifact failed: {0}")]
    Artifact(String),
}

pub struct ReviewRequest {
    pub issue: TrackerIssue,
    pub prompt: String,
    pub workspace: PathBuf,
    pub artifact_root: PathBuf,
}

impl ReviewJob {
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
            report: None,
            error: Some(error.into()),
        }
    }
}

pub trait ReviewBackend {
    fn kind(&self) -> &'static str;
    fn start(&self, request: ReviewRequest) -> Result<ReviewJob, ReviewError>;
    fn poll(&self, job: ReviewJob) -> Result<ReviewJob, ReviewError>;
    fn cancel(&self, _job: &ReviewJob) -> Result<(), ReviewError> {
        Ok(())
    }
}

pub fn review_job_is_terminal(job: &ReviewJob) -> bool {
    matches!(
        job.state,
        ReviewJobState::Completed
            | ReviewJobState::Failed
            | ReviewJobState::TimedOut
            | ReviewJobState::Cancelled
    )
}

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
            backend.cancel(&job)?;
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
