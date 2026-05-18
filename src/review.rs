use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::agent::{classify_usage_limit_text, UsageLimitPause};
use crate::lane_claim::{LaneClaim, LaneClaimState};
use crate::model::{normalize_state, TrackerIssue};
use crate::workspace::safe_identifier;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewFindingClass {
    Confirmed,
    Plausible,
    Rejected,
    NeedsContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFinding {
    pub class: ReviewFindingClass,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentReviewReport {
    pub reviewer_backend: String,
    #[serde(default)]
    pub findings: Vec<ReviewFinding>,
    pub summary: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

impl AgentReviewReport {
    pub fn blocks_progress(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.class == ReviewFindingClass::Confirmed)
    }

    pub fn is_inconclusive(&self) -> bool {
        self.inconclusive_reason().is_some()
    }

    pub fn inconclusive_reason(&self) -> Option<String> {
        if self
            .findings
            .iter()
            .any(|finding| finding.class == ReviewFindingClass::NeedsContext)
        {
            return Some("review produced Needs Context findings".into());
        }

        let text = [
            self.summary.as_deref(),
            self.stdout.as_deref(),
            self.stderr.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");

        inconclusive_review_text_reason(&text)
    }

    pub fn blocks_human_review(&self) -> bool {
        self.blocks_progress()
    }
}

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
    pub artifact_path: Option<PathBuf>,
    pub ledger_path: PathBuf,
    pub decision_outcome: ReviewOutcome,
    pub decision_target_state: Option<String>,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub finding_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewOutcome {
    PassedToHumanReview,
    NeedsRework,
    InconclusiveNeedsRework,
    NeedsHumanInput,
    StillRunning,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewGateDecision {
    pub outcome: ReviewOutcome,
    pub target_state: Option<&'static str>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewStaleReason {
    MergeConflict,
    BaseBranchUpdated,
    ReviewOutdated,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewReworkClass {
    MechanicalConflictResolution,
    BaseRefresh,
    SemanticChange,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewFreshnessDecisionKind {
    PriorReviewStillValid,
    PriorReviewInvalidated,
    NeedsHumanInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFreshnessInput {
    pub issue_ref: String,
    pub prior_head_sha: String,
    pub current_head_sha: String,
    pub prior_base_sha: String,
    pub current_base_sha: String,
    #[serde(default)]
    pub changed_files: Vec<String>,
    pub stale_reason: ReviewStaleReason,
    pub rework_class: ReviewReworkClass,
    pub patch_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFreshnessDecision {
    pub kind: ReviewFreshnessDecisionKind,
    pub prior_human_review_valid: bool,
    pub human_rereview_required: bool,
    pub main_agent_target_state: String,
    pub authorized_next_state: Option<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFreshnessReport {
    pub input: ReviewFreshnessInput,
    pub decision: ReviewFreshnessDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewRunEligibility {
    Eligible { worker_key: String },
    AlreadyQueued { worker_key: String },
    NotInAgentReview { current_state: String },
    InvalidHandoff { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewActor {
    MainImplementationAgent,
    IndependentReviewAgent,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeReviewOutcome {
    Pass,
    ConfirmedFinding,
    Failed,
}

#[derive(Debug, Clone)]
pub struct FakeReviewBackend {
    outcome: FakeReviewOutcome,
}

impl FakeReviewBackend {
    pub fn new(outcome: FakeReviewOutcome) -> Self {
        Self { outcome }
    }
}

impl ReviewBackend for FakeReviewBackend {
    fn kind(&self) -> &'static str {
        "fake-reviewer"
    }

    fn start(&self, request: ReviewRequest) -> Result<ReviewJob, ReviewError> {
        Ok(ReviewJob {
            id: review_job_id("fake"),
            issue_ref: request.issue.identifier,
            backend: self.kind().into(),
            state: ReviewJobState::Queued,
            artifact_path: None,
            ledger_path: None,
            report: None,
            error: None,
        })
    }

    fn poll(&self, mut job: ReviewJob) -> Result<ReviewJob, ReviewError> {
        match self.outcome {
            FakeReviewOutcome::Pass => {
                job.state = ReviewJobState::Completed;
                job.report = Some(AgentReviewReport {
                    reviewer_backend: self.kind().into(),
                    findings: Vec::new(),
                    summary: Some("Fake reviewer found no confirmed findings.".into()),
                    stdout: None,
                    stderr: None,
                });
            }
            FakeReviewOutcome::ConfirmedFinding => {
                job.state = ReviewJobState::Completed;
                job.report = Some(AgentReviewReport {
                    reviewer_backend: self.kind().into(),
                    findings: vec![ReviewFinding {
                        class: ReviewFindingClass::Confirmed,
                        title: "Confirmed fake finding".into(),
                        body: "Fake reviewer was configured to require rework.".into(),
                    }],
                    summary: Some("Fake reviewer produced one confirmed finding.".into()),
                    stdout: None,
                    stderr: None,
                });
            }
            FakeReviewOutcome::Failed => {
                job.state = ReviewJobState::Failed;
                job.error = Some("Fake reviewer failure.".into());
            }
        }
        Ok(job)
    }
}

#[derive(Debug, Clone)]
pub struct GeminiCliReviewBackend {
    command: String,
    children: Arc<Mutex<BTreeMap<String, Child>>>,
}

impl GeminiCliReviewBackend {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            children: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

impl ReviewBackend for GeminiCliReviewBackend {
    fn kind(&self) -> &'static str {
        "gemini-cli"
    }

    fn start(&self, request: ReviewRequest) -> Result<ReviewJob, ReviewError> {
        fs::create_dir_all(&request.workspace)
            .map_err(|error| ReviewError::Artifact(error.to_string()))?;
        fs::create_dir_all(&request.artifact_root)
            .map_err(|error| ReviewError::Artifact(error.to_string()))?;
        let id = review_job_id("gemini");
        let prompt_path = request.artifact_root.join(format!("{id}.prompt.md"));
        fs::write(&prompt_path, &request.prompt)
            .map_err(|error| ReviewError::Artifact(error.to_string()))?;

        let mut child = Command::new(&self.command)
            .current_dir(&request.workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                ReviewError::Backend(diagnose_gemini_spawn_failure(&self.command, &error))
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(request.prompt.as_bytes())
                .map_err(|error| ReviewError::Backend(error.to_string()))?;
        }

        self.children
            .lock()
            .map_err(|error| ReviewError::Backend(error.to_string()))?
            .insert(id.clone(), child);

        Ok(ReviewJob {
            id,
            issue_ref: request.issue.identifier,
            backend: self.kind().into(),
            state: ReviewJobState::Running,
            artifact_path: Some(prompt_path),
            ledger_path: None,
            report: None,
            error: None,
        })
    }

    fn poll(&self, mut job: ReviewJob) -> Result<ReviewJob, ReviewError> {
        if job.state != ReviewJobState::Running {
            return Ok(job);
        }

        let mut children = self
            .children
            .lock()
            .map_err(|error| ReviewError::Backend(error.to_string()))?;
        let Some(child) = children.get_mut(&job.id) else {
            job.state = ReviewJobState::Failed;
            job.error = Some("Gemini child process was not found.".into());
            return Ok(job);
        };

        if child
            .try_wait()
            .map_err(|error| ReviewError::Backend(error.to_string()))?
            .is_none()
        {
            return Ok(job);
        }

        let child = children
            .remove(&job.id)
            .expect("child existed after successful lookup");
        let output = child
            .wait_with_output()
            .map_err(|error| ReviewError::Backend(error.to_string()))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            let findings = classify_findings(&stdout);
            job.state = ReviewJobState::Completed;
            job.report = Some(AgentReviewReport {
                reviewer_backend: self.kind().into(),
                findings,
                summary: Some(
                    first_non_empty_line(&stdout)
                        .unwrap_or("Review completed.")
                        .into(),
                ),
                stdout: Some(stdout),
                stderr: Some(stderr),
            });
        } else {
            job.state = ReviewJobState::Failed;
            let status = output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "terminated by signal".into());
            let detail = stderr.trim();
            job.error = Some(if detail.is_empty() {
                format!("Gemini review command exited with status {status} and no stderr.")
            } else {
                format!("Gemini review command exited with status {status}: {detail}")
            });
        }

        Ok(job)
    }

    fn cancel(&self, job: &ReviewJob) -> Result<(), ReviewError> {
        let mut children = self
            .children
            .lock()
            .map_err(|error| ReviewError::Backend(error.to_string()))?;
        if let Some(mut child) = children.remove(&job.id) {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}

fn diagnose_gemini_spawn_failure(command: &str, error: &std::io::Error) -> String {
    match error.kind() {
        ErrorKind::NotFound if command_uses_path_lookup(command) => format!(
            "review backend startup failed: configured command: `{command}`; resolved executable: not found in worker PATH; suggested fix: configure `review.gemini_command` with an absolute Gemini path such as `/opt/homebrew/bin/gemini`, or export a worker PATH that can resolve `{command}`; retry: rerun `review loop` after updating the workflow or environment."
        ),
        ErrorKind::NotFound => format!(
            "review backend startup failed: configured command: `{command}`; resolved executable: path was not found or could not be executed; suggested fix: verify the configured Gemini path exists and is executable; retry: rerun `review loop` after updating the workflow or environment."
        ),
        ErrorKind::PermissionDenied => format!(
            "review backend startup failed: configured command: `{command}`; resolved executable: permission denied; suggested fix: make the Gemini command executable or configure `review.gemini_command` to an executable path; retry: rerun `review loop` after fixing permissions."
        ),
        _ => format!(
            "review backend startup failed: configured command: `{command}`; spawn error: {error}; suggested fix: inspect the Gemini CLI installation, auth/configuration, and worker environment; retry: rerun `review loop` after fixing the backend."
        ),
    }
}

fn command_uses_path_lookup(command: &str) -> bool {
    let path = Path::new(command);
    !path.is_absolute() && !command.contains(std::path::MAIN_SEPARATOR)
}

pub fn classify_findings(output: &str) -> Vec<ReviewFinding> {
    output
        .lines()
        .filter_map(parse_finding_line)
        .collect::<Vec<_>>()
}

pub fn main_agent_completion_decision() -> ReviewGateDecision {
    ReviewGateDecision {
        outcome: ReviewOutcome::StillRunning,
        target_state: Some("agent_review"),
        message:
            "Main implementation agent completed local work; independent Agent Review is required."
                .into(),
    }
}

pub fn review_gate_decision(job: &ReviewJob) -> ReviewGateDecision {
    review_gate_decision_for_actor(job, ReviewActor::IndependentReviewAgent)
}

pub fn review_gate_decision_for_actor(job: &ReviewJob, actor: ReviewActor) -> ReviewGateDecision {
    if actor == ReviewActor::MainImplementationAgent {
        return main_agent_completion_decision();
    }

    match job.state {
        ReviewJobState::Queued | ReviewJobState::Running => ReviewGateDecision {
            outcome: ReviewOutcome::StillRunning,
            target_state: Some("agent_review"),
            message: "Agent review is still running; issue remains in Agent Review.".into(),
        },
        ReviewJobState::Completed => match &job.report {
            Some(report) if report.blocks_progress() => ReviewGateDecision {
                outcome: ReviewOutcome::NeedsRework,
                target_state: Some("rework"),
                message: "Confirmed Agent Review findings require Rework.".into(),
            },
            Some(report) if report.is_inconclusive() => ReviewGateDecision {
                outcome: ReviewOutcome::InconclusiveNeedsRework,
                target_state: Some("rework"),
                message: format!(
                    "Agent Review was inconclusive and requires Rework: {}.",
                    report
                        .inconclusive_reason()
                        .unwrap_or_else(|| "review could not complete with durable evidence".into())
                ),
            },
            Some(_) => ReviewGateDecision {
                outcome: ReviewOutcome::PassedToHumanReview,
                target_state: Some("human_review"),
                message: "Independent Agent Review passed with recorded evidence; issue is ready for Human Review.".into(),
            },
            None => ReviewGateDecision {
                outcome: ReviewOutcome::NeedsHumanInput,
                target_state: Some("need_human_input"),
                message: "Agent review completed without a report.".into(),
            },
        },
        ReviewJobState::Failed | ReviewJobState::TimedOut
            if review_required_operator_actions(job).is_some() =>
        {
            ReviewGateDecision {
                outcome: ReviewOutcome::StillRunning,
                target_state: Some("agent_review"),
                message:
                    "Agent Review backend is blocked by required operator action; issue remains in Agent Review."
                        .into(),
            }
        }
        ReviewJobState::Failed | ReviewJobState::TimedOut => ReviewGateDecision {
            outcome: ReviewOutcome::NeedsHumanInput,
            target_state: Some("need_human_input"),
            message: "Agent review failed or timed out; human input is required.".into(),
        },
        ReviewJobState::Cancelled => ReviewGateDecision {
            outcome: ReviewOutcome::Cancelled,
            target_state: Some("agent_review"),
            message: "Agent review was cancelled; issue remains in Agent Review.".into(),
        },
    }
}

pub fn review_worker_key(issue: &TrackerIssue, backend: &str) -> String {
    format!(
        "review:{}:{}",
        issue.identifier.trim(),
        backend.trim().to_lowercase()
    )
}

pub fn review_run_eligibility(
    issue: &TrackerIssue,
    agent_review_state: &str,
    backend: &str,
) -> ReviewRunEligibility {
    if issue.normalized_state() != normalize_state(agent_review_state) {
        return ReviewRunEligibility::NotInAgentReview {
            current_state: issue.state.clone(),
        };
    }

    if issue.linked_pull_requests.is_empty() {
        return ReviewRunEligibility::InvalidHandoff {
            reason: "Agent Review handoff has no verified Project-visible linked PR; Main Agent or operator repair must establish the PR relationship before normal review.".into(),
        };
    }

    if issue
        .linked_pull_requests
        .iter()
        .any(|pr| pr.is_draft == Some(true))
    {
        return ReviewRunEligibility::InvalidHandoff {
            reason: "Agent Review handoff has a linked draft PR; Main Agent must mark it ready before normal review.".into(),
        };
    }

    let worker_key = review_worker_key(issue, backend);
    if has_active_review_worker(issue, &worker_key) {
        ReviewRunEligibility::AlreadyQueued { worker_key }
    } else {
        ReviewRunEligibility::Eligible { worker_key }
    }
}

fn has_active_review_worker(issue: &TrackerIssue, worker_key: &str) -> bool {
    let terminal_failure_marker = issue
        .description
        .as_deref()
        .is_some_and(|description| terminal_review_failure_marker_matches(description, worker_key));

    issue.project_fields.iter().any(|(key, value)| {
        let key = key.to_lowercase();
        if !key.contains("review") {
            return false;
        }
        let is_review_agent_field = key == "review agent";

        let value = value
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string());
        active_review_marker_matches(&value, worker_key)
            || (is_review_agent_field
                && !terminal_failure_marker
                && structured_active_review_claim(&value))
            || (is_review_agent_field
                && !terminal_failure_marker
                && fixed_review_agent_claim_matches(&value, worker_key))
    }) || issue
        .description
        .as_deref()
        .is_some_and(|description| active_review_marker_matches(description, worker_key))
}

fn structured_active_review_claim(value: &str) -> bool {
    LaneClaim::parse(value)
        .map(|claim| claim.lane.as_str() == "review" && claim.state == LaneClaimState::Active)
        .unwrap_or(false)
}

fn fixed_review_agent_claim_matches(value: &str, worker_key: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    if value == "hold" {
        return true;
    }
    worker_key.to_ascii_lowercase().contains(":gemini")
        && matches!(value.as_str(), "gemini a" | "gemini b")
}

fn active_review_marker_matches(value: &str, worker_key: &str) -> bool {
    let value = value.to_lowercase();
    let worker_key = worker_key.to_lowercase();
    if !value.contains(&worker_key) {
        return false;
    }
    if terminal_review_failure_marker_matches(&value, &worker_key) {
        return false;
    }

    value.contains("queued")
        || value.contains("job state: running")
        || value.contains("review worker running")
        || value.contains("running review:")
}

fn terminal_review_failure_marker_matches(value: &str, worker_key: &str) -> bool {
    let value = value.to_lowercase();
    let worker_key = worker_key.to_lowercase();
    value.contains(&worker_key)
        && (value.contains("job state: failed")
            || value.contains("job state: timedout")
            || value.contains("job state: timed out")
            || value.contains("job state: cancelled"))
        && (value.contains("required operator action")
            || value.contains("review backend")
            || value.contains("gemini review command")
            || value.contains("retry: rerun `review loop`")
            || value.contains("retry: rerun review loop"))
}

pub fn render_review_workpad(issue: &TrackerIssue, job: &ReviewJob) -> String {
    let decision = review_gate_decision_for_actor(job, ReviewActor::IndependentReviewAgent);
    let mut lines = vec![
        "## Agent Review".to_string(),
        String::new(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Worker key: {}", review_worker_key(issue, &job.backend)),
        format!("- Reviewer backend: {}", job.backend),
        format!("- Job state: {:?}", job.state),
        format!("- Decision: {}", decision.message),
    ];

    if let Some(path) = &job.artifact_path {
        lines.push(format!("- Artifact: {}", path.display()));
    }
    if let Some(path) = &job.ledger_path {
        lines.push(format!("- Review job ledger: {}", path.display()));
    }
    if let Some(error) = &job.error {
        lines.push(format!("- Error: {error}"));
    }
    if let Some(actions) = review_required_operator_actions(job) {
        lines.push(String::new());
        lines.push("### Required Operator Action".into());
        lines.extend(actions);
    }
    if let Some(pause) = review_usage_limit_pause(job) {
        lines.push(format!("- Usage-limit classifier: `{}`", pause.classifier));
        lines.push(format!("- Usage-limit evidence: {}", pause.evidence));
        lines.push("- Review did not pass; unavailable or inconclusive review must not move to Human Review.".into());
    }
    if let Some(report) = &job.report {
        if let Some(reason) = report.inconclusive_reason() {
            lines.push(String::new());
            lines.push("### Inconclusive Review Diagnostic".into());
            lines.push(format!("- Reason: {reason}"));
            lines.push(
                "- Automatic Review Agent output did not establish a conclusive pass.".into(),
            );
            lines.push("- Route to `Rework`; do not move to `Human Review`.".into());
        }
    }

    lines.push(String::new());
    lines.push("### Findings".into());
    match &job.report {
        Some(report) if report.findings.is_empty() => {
            lines.push("- No confirmed, plausible, rejected, or needs-context findings.".into());
        }
        Some(report) => {
            for finding in &report.findings {
                lines.push(format!(
                    "- {:?}: {} - {}",
                    finding.class, finding.title, finding.body
                ));
            }
        }
        None => lines.push("- No report captured yet.".into()),
    }

    if decision.outcome == ReviewOutcome::PassedToHumanReview {
        lines.push(String::new());
        lines.push("- Review pass evidence: `recorded`".into());
        lines.push("Evidence recorded. Independent Review Agent may move this issue to Human Review; the main implementation agent must not.".into());
    }

    lines.join("\n")
}

fn review_required_operator_actions(job: &ReviewJob) -> Option<Vec<String>> {
    let error = job.error.as_deref().unwrap_or_default();
    if job.state == ReviewJobState::TimedOut {
        return Some(vec![
            "- Review backend timed out before producing evidence.".into(),
            "- Check Gemini auth/configuration and increase `review.timeout_ms` only if the backend is healthy but slow.".into(),
            "- Retry: rerun `review loop` for this issue after fixing the backend.".into(),
        ]);
    }

    if job.state != ReviewJobState::Failed {
        return None;
    }

    if error.contains("review backend startup failed")
        || error.contains("not found in worker PATH")
        || error.contains("permission denied")
    {
        return Some(vec![
            "- Fix the Review Agent backend command or worker PATH shown in the error above.".into(),
            "- For Gemini CLI, prefer an absolute `review.gemini_command` path or export a worker PATH that resolves `gemini`.".into(),
            "- This issue must not move to `Human Review` until an independent Review Agent records passing review evidence.".into(),
            "- Retry: rerun `review loop` for this issue after updating the workflow or environment.".into(),
        ]);
    }

    if error.contains("exited with status") {
        return Some(vec![
            "- The Review Agent backend started but exited unsuccessfully.".into(),
            "- Inspect stderr/auth/configuration from the error above; do not move to Human Review until a review pass is recorded.".into(),
            "- This issue must not move to `Human Review` until an independent Review Agent records passing review evidence.".into(),
            "- Retry: rerun `review loop` for this issue after fixing the backend.".into(),
        ]);
    }

    None
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
    let decision = review_gate_decision_for_actor(job, ReviewActor::IndependentReviewAgent);
    ReviewJobLedgerRecord {
        issue_ref: issue.identifier.clone(),
        issue_title: issue.title.clone(),
        job_id: job.id.clone(),
        worker_key: review_worker_key(issue, &job.backend),
        backend: job.backend.clone(),
        state: job.state.clone(),
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

pub fn classify_review_freshness(input: ReviewFreshnessInput) -> ReviewFreshnessReport {
    let decision = match input.rework_class {
        ReviewReworkClass::MechanicalConflictResolution | ReviewReworkClass::BaseRefresh => {
            ReviewFreshnessDecision {
                kind: ReviewFreshnessDecisionKind::PriorReviewStillValid,
                prior_human_review_valid: true,
                human_rereview_required: false,
                main_agent_target_state: "agent_review".into(),
                authorized_next_state: Some("merging".into()),
                rationale: "Rework is classified as mechanical; prior Human Review can be preserved when evidence is recorded.".into(),
            }
        }
        ReviewReworkClass::SemanticChange => ReviewFreshnessDecision {
            kind: ReviewFreshnessDecisionKind::PriorReviewInvalidated,
            prior_human_review_valid: false,
            human_rereview_required: true,
            main_agent_target_state: "agent_review".into(),
            authorized_next_state: Some("agent_review".into()),
            rationale: "Semantic implementation changes invalidate prior Human Review and require the normal Agent Review then Human Review path.".into(),
        },
        ReviewReworkClass::Unknown => ReviewFreshnessDecision {
            kind: ReviewFreshnessDecisionKind::NeedsHumanInput,
            prior_human_review_valid: false,
            human_rereview_required: true,
            main_agent_target_state: "agent_review".into(),
            authorized_next_state: Some("need_human_input".into()),
            rationale: "Rework class is unknown, so prior review freshness cannot be safely preserved.".into(),
        },
    };

    ReviewFreshnessReport { input, decision }
}

pub fn render_review_freshness_workpad(report: &ReviewFreshnessReport) -> String {
    let input = &report.input;
    let decision = &report.decision;
    let mut lines = vec![
        "## Review Freshness".to_string(),
        String::new(),
        format!("- Issue: {}", input.issue_ref),
        format!("- Stale reason: {:?}", input.stale_reason),
        format!("- Rework class: {:?}", input.rework_class),
        format!("- Prior head SHA: `{}`", input.prior_head_sha),
        format!("- Current head SHA: `{}`", input.current_head_sha),
        format!("- Prior base SHA: `{}`", input.prior_base_sha),
        format!("- Current base SHA: `{}`", input.current_base_sha),
        format!(
            "- Prior Human Review still valid: `{}`",
            decision.prior_human_review_valid
        ),
        format!(
            "- Human re-review required: `{}`",
            decision.human_rereview_required
        ),
        format!(
            "- Main-agent target state: `{}`",
            decision.main_agent_target_state
        ),
        format!(
            "- Authorized next state after review freshness evidence: `{}`",
            decision.authorized_next_state.as_deref().unwrap_or("none")
        ),
        format!("- Decision: {:?}", decision.kind),
        format!("- Rationale: {}", decision.rationale),
    ];

    lines.push(String::new());
    lines.push("### Changed Files".into());
    if input.changed_files.is_empty() {
        lines.push("- None recorded.".into());
    } else {
        lines.extend(input.changed_files.iter().map(|file| format!("- `{file}`")));
    }

    lines.push(String::new());
    lines.push("### Patch Summary".into());
    lines.push(
        input
            .patch_summary
            .as_deref()
            .filter(|summary| !summary.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "Not recorded.".into()),
    );

    lines.push(String::new());
    lines.push("### Authority Boundary".into());
    lines.push("- This freshness report is evidence, not an automatic approval.".into());
    lines.push("- Main implementation agent still stops at `Agent Review`.".into());
    lines.push("- `Human Review` remains reserved for an independent Review Agent or human-authorized workflow.".into());

    lines.join("\n")
}

pub fn transition_allowed_for_main_agent(normalized_state: &str) -> bool {
    !matches!(normalized_state, "human_review" | "human review")
}

pub fn transition_allowed_for_review_agent(
    normalized_state: &str,
    decision: &ReviewGateDecision,
) -> bool {
    match normalized_state {
        "human_review" | "human review" => decision.outcome == ReviewOutcome::PassedToHumanReview,
        "rework" => matches!(
            decision.outcome,
            ReviewOutcome::NeedsRework | ReviewOutcome::InconclusiveNeedsRework
        ),
        "need_human_input" | "need human input" => {
            decision.outcome == ReviewOutcome::NeedsHumanInput
        }
        "agent_review" | "agent review" => {
            matches!(
                decision.outcome,
                ReviewOutcome::StillRunning | ReviewOutcome::Cancelled
            )
        }
        _ => true,
    }
}

fn parse_finding_line(line: &str) -> Option<ReviewFinding> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') {
        return None;
    }

    let closing_bracket = trimmed.find(']')?;
    let label = trimmed[1..closing_bracket]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let class = match label.as_str() {
        "confirmed" => ReviewFindingClass::Confirmed,
        "plausible" => ReviewFindingClass::Plausible,
        "rejected" => ReviewFindingClass::Rejected,
        "needs context" => ReviewFindingClass::NeedsContext,
        _ => return None,
    };

    let rest = trimmed[closing_bracket + 1..].trim();
    let (title, body) = rest.split_once(':').unwrap_or((rest, ""));
    Some(ReviewFinding {
        class,
        title: title.trim().to_string(),
        body: body.trim().to_string(),
    })
}

fn inconclusive_review_text_reason(text: &str) -> Option<String> {
    let normalized = text
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return None;
    }

    let missing_evidence_patterns = [
        "workspace is empty",
        "empty workspace",
        "missing workspace",
        "workspace was missing",
        "missing pr evidence",
        "pr evidence was missing",
        "pr evidence is missing",
        "missing pull request evidence",
        "pull request evidence was missing",
        "pull request evidence is missing",
        "missing handoff evidence",
        "handoff evidence was missing",
        "handoff evidence is missing",
        "missing code changes",
        "code changes were missing",
        "code changes are missing",
        "expected code changes were missing",
        "expected code changes are missing",
        "no code changes",
        "no diff",
        "no pull request evidence",
    ];
    if let Some(pattern) = missing_evidence_patterns
        .iter()
        .find(|pattern| normalized.contains(**pattern))
    {
        return Some(format!("automatic review reported {pattern}"));
    }

    let unable_to_review_patterns = [
        "unable to complete",
        "could not complete",
        "cannot complete",
        "could not be completed",
        "unable to inspect",
        "could not inspect",
        "cannot inspect",
        "unable to review",
        "could not review",
        "cannot review",
        "inconclusive review",
        "review is inconclusive",
        "review was inconclusive",
    ];
    if let Some(pattern) = unable_to_review_patterns
        .iter()
        .find(|pattern| normalized.contains(**pattern))
    {
        return Some(format!("automatic review output said it was {pattern}"));
    }

    None
}

static REVIEW_JOB_ID_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn review_job_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = REVIEW_JOB_ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{millis}-{sequence}")
}

fn first_non_empty_line(output: &str) -> Option<&str> {
    output.lines().find(|line| !line.trim().is_empty())
}

#[allow(dead_code)]
fn artifact_path(root: &Path, id: &str, extension: &str) -> PathBuf {
    root.join(format!("{id}.{extension}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue() -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: "issue-1".into(),
            item_id: None,
            identifier: "#1".into(),
            title: "Review me".into(),
            description: Some("body".into()),
            url: None,
            state: "Agent Review".into(),
            labels: vec![],
            assignees: vec![],
            priority: None,
            branch_name: None,
            linked_pull_requests: vec![crate::model::LinkedPullRequest {
                url: Some("https://github.com/Alive24/jade-symphony/pull/1".into()),
                state: Some("OPEN".into()),
                is_draft: Some(false),
                ..Default::default()
            }],
            blocked_by: vec![],
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    fn completed_gemini_review(output: &str) -> ReviewJob {
        ReviewJob {
            id: "gemini-1".into(),
            issue_ref: "#1".into(),
            backend: "gemini-cli".into(),
            state: ReviewJobState::Completed,
            artifact_path: Some("/tmp/reviews/gemini.prompt.md".into()),
            ledger_path: None,
            report: Some(AgentReviewReport {
                reviewer_backend: "gemini-cli".into(),
                findings: classify_findings(output),
                summary: first_non_empty_line(output).map(str::to_string),
                stdout: Some(output.into()),
                stderr: Some(String::new()),
            }),
            error: None,
        }
    }

    fn review_request_with_temp_roots(temp: &tempfile::TempDir) -> ReviewRequest {
        ReviewRequest {
            issue: issue(),
            prompt: "review".into(),
            workspace: temp.path().join("review-workspace"),
            artifact_root: temp.path().join("review-artifacts"),
        }
    }

    fn poll_test_job_until_terminal(
        backend: &dyn ReviewBackend,
        job: ReviewJob,
    ) -> Result<ReviewJob, ReviewError> {
        poll_review_job_until_terminal(
            backend,
            job,
            Duration::from_secs(5),
            Duration::from_millis(10),
        )
    }

    #[derive(Clone)]
    struct DelayedBackend {
        polls: Arc<Mutex<usize>>,
        cancels: Arc<Mutex<usize>>,
        complete_after: usize,
    }

    impl DelayedBackend {
        fn new(complete_after: usize) -> Self {
            Self {
                polls: Arc::new(Mutex::new(0)),
                cancels: Arc::new(Mutex::new(0)),
                complete_after,
            }
        }

        fn poll_count(&self) -> usize {
            *self.polls.lock().unwrap()
        }

        fn cancel_count(&self) -> usize {
            *self.cancels.lock().unwrap()
        }
    }

    impl ReviewBackend for DelayedBackend {
        fn kind(&self) -> &'static str {
            "delayed"
        }

        fn start(&self, request: ReviewRequest) -> Result<ReviewJob, ReviewError> {
            Ok(ReviewJob {
                id: "delayed-1".into(),
                issue_ref: request.issue.identifier,
                backend: self.kind().into(),
                state: ReviewJobState::Running,
                artifact_path: None,
                ledger_path: None,
                report: None,
                error: None,
            })
        }

        fn poll(&self, mut job: ReviewJob) -> Result<ReviewJob, ReviewError> {
            let mut polls = self.polls.lock().unwrap();
            *polls += 1;
            if *polls >= self.complete_after {
                job.state = ReviewJobState::Completed;
                job.report = Some(AgentReviewReport {
                    reviewer_backend: self.kind().into(),
                    findings: Vec::new(),
                    summary: Some("Delayed review completed.".into()),
                    stdout: Some("Delayed review completed.".into()),
                    stderr: Some(String::new()),
                });
            }
            Ok(job)
        }

        fn cancel(&self, _job: &ReviewJob) -> Result<(), ReviewError> {
            *self.cancels.lock().unwrap() += 1;
            Ok(())
        }
    }

    #[test]
    fn poll_review_job_until_terminal_waits_for_delayed_completion() {
        let backend = DelayedBackend::new(3);
        let temp = tempfile::tempdir().unwrap();
        let request = review_request_with_temp_roots(&temp);
        let job = backend.start(request).unwrap();

        let job = poll_review_job_until_terminal(
            &backend,
            job,
            Duration::from_secs(1),
            Duration::from_millis(0),
        )
        .unwrap();

        assert_eq!(job.state, ReviewJobState::Completed);
        assert_eq!(backend.poll_count(), 3);
        assert_eq!(backend.cancel_count(), 0);
        assert_eq!(
            job.report
                .as_ref()
                .and_then(|report| report.summary.as_deref()),
            Some("Delayed review completed.")
        );
    }

    #[test]
    fn poll_review_job_until_terminal_times_out_and_cancels_running_job() {
        let backend = DelayedBackend::new(usize::MAX);
        let temp = tempfile::tempdir().unwrap();
        let request = review_request_with_temp_roots(&temp);
        let job = backend.start(request).unwrap();

        let job = poll_review_job_until_terminal(
            &backend,
            job,
            Duration::from_millis(0),
            Duration::from_millis(0),
        )
        .unwrap();

        assert_eq!(job.state, ReviewJobState::TimedOut);
        assert_eq!(backend.poll_count(), 1);
        assert_eq!(backend.cancel_count(), 1);
        assert_eq!(
            job.error.as_deref(),
            Some("Review backend timed out after 0ms.")
        );
    }

    #[test]
    fn gemini_backend_missing_path_command_has_actionable_startup_diagnostic() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let backend = GeminiCliReviewBackend::new("jade-missing-gemini-command");
        let request = ReviewRequest {
            issue: issue(),
            prompt: "Review this".into(),
            workspace,
            artifact_root: temp.path().join("reviews"),
        };

        let error = backend.start(request).unwrap_err().to_string();

        assert!(error.contains("review backend startup failed"));
        assert!(error.contains("configured command: `jade-missing-gemini-command`"));
        assert!(error.contains("resolved executable: not found in worker PATH"));
        assert!(error.contains("absolute Gemini path"));
        assert!(error.contains("retry: rerun `review loop`"));
    }

    #[test]
    fn review_workpad_includes_required_operator_action_for_backend_startup_failure() {
        let job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "review backend failed: review backend startup failed: configured command: `gemini`; resolved executable: not found in worker PATH; suggested fix: configure `review.gemini_command` with an absolute Gemini path; retry: rerun `review loop` after updating the workflow or environment.",
        );

        let workpad = render_review_workpad(&issue(), &job);

        assert!(workpad.contains("### Required Operator Action"));
        assert!(workpad.contains("Fix the Review Agent backend command or worker PATH"));
        assert!(workpad.contains("absolute `review.gemini_command` path"));
        assert!(workpad.contains("Retry: rerun `review loop`"));
        assert!(workpad.contains("must not"));
    }

    #[test]
    fn actionable_backend_startup_failure_remains_in_agent_review() {
        let job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "review backend failed: review backend startup failed: configured command: `gemini`; resolved executable: not found in worker PATH",
        );

        let decision = review_gate_decision(&job);

        assert_eq!(decision.outcome, ReviewOutcome::StillRunning);
        assert_eq!(decision.target_state, Some("agent_review"));
        assert!(decision.message.contains("required operator action"));
    }

    #[test]
    fn review_workpad_includes_operator_action_for_nonzero_backend_exit() {
        let mut job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "Gemini review command exited with status 1: auth required",
        );
        job.id = "gemini-1".into();

        let workpad = render_review_workpad(&issue(), &job);

        assert!(workpad.contains("### Required Operator Action"));
        assert!(workpad.contains("started but exited unsuccessfully"));
        assert!(workpad.contains("Inspect stderr/auth/configuration"));
    }

    #[test]
    fn classifies_bootstrap_finding_categories() {
        let findings = classify_findings(
            "[Confirmed] Bug: broken\n[Plausible] Risk: maybe\n[Rejected] Noise: no\n[Needs Context] Question: unclear",
        );

        assert_eq!(findings.len(), 4);
        assert_eq!(findings[0].class, ReviewFindingClass::Confirmed);
        assert_eq!(findings[1].class, ReviewFindingClass::Plausible);
        assert_eq!(findings[2].class, ReviewFindingClass::Rejected);
        assert_eq!(findings[3].class, ReviewFindingClass::NeedsContext);
    }

    #[test]
    fn classifies_finding_categories_with_case_and_spacing_variations() {
        let findings = classify_findings(
            "[confirmed] Bug: broken\n[ PLAUSIBLE ] Risk: maybe\n[rejected] Noise: no\n[Needs   Context] Question: unclear",
        );

        assert_eq!(findings.len(), 4);
        assert_eq!(findings[0].class, ReviewFindingClass::Confirmed);
        assert_eq!(findings[1].class, ReviewFindingClass::Plausible);
        assert_eq!(findings[2].class, ReviewFindingClass::Rejected);
        assert_eq!(findings[3].class, ReviewFindingClass::NeedsContext);
    }

    #[test]
    fn confirmed_findings_route_to_rework() {
        let backend = FakeReviewBackend::new(FakeReviewOutcome::ConfirmedFinding);
        let temp = tempfile::tempdir().unwrap();
        let request = review_request_with_temp_roots(&temp);
        let job = backend.poll(backend.start(request).unwrap()).unwrap();
        let decision = review_gate_decision(&job);

        assert_eq!(decision.outcome, ReviewOutcome::NeedsRework);
        assert_eq!(decision.target_state, Some("rework"));
    }

    #[test]
    fn review_agent_passed_review_moves_to_human_review() {
        let backend = FakeReviewBackend::new(FakeReviewOutcome::Pass);
        let temp = tempfile::tempdir().unwrap();
        let request = review_request_with_temp_roots(&temp);
        let job = backend.poll(backend.start(request).unwrap()).unwrap();
        let decision = review_gate_decision(&job);

        assert_eq!(decision.outcome, ReviewOutcome::PassedToHumanReview);
        assert_eq!(decision.target_state, Some("human_review"));
        assert!(transition_allowed_for_review_agent(
            "human_review",
            &decision
        ));
    }

    #[test]
    fn gemini_backend_creates_missing_review_workspace_before_launch() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("missing-review-workspace");
        let artifact_root = temp.path().join("reviews");
        let reviewer = temp.path().join("reviewer.sh");
        fs::write(&reviewer, "#!/bin/sh\nprintf 'Review completed.\\n'\n").unwrap();
        let mut permissions = fs::metadata(&reviewer).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&reviewer, permissions).unwrap();

        let backend = GeminiCliReviewBackend::new(reviewer.display().to_string());
        let request = ReviewRequest {
            issue: issue(),
            prompt: "Review completed.".into(),
            workspace: workspace.clone(),
            artifact_root,
        };

        let job = backend.start(request).unwrap();
        assert!(workspace.is_dir());
        let job = poll_test_job_until_terminal(&backend, job).unwrap();

        assert_eq!(job.state, ReviewJobState::Completed);
        assert_eq!(
            job.report
                .as_ref()
                .and_then(|report| report.summary.as_deref()),
            Some("Review completed.")
        );
    }

    #[test]
    fn gemini_backend_closes_prompt_stdin_after_launch() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("review-workspace");
        let artifact_root = temp.path().join("reviews");
        let reviewer = temp.path().join("reviewer.sh");
        fs::write(
            &reviewer,
            "#!/bin/sh\ncat >/dev/null\nprintf 'Review completed after EOF.\\n'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&reviewer).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&reviewer, permissions).unwrap();

        let backend = GeminiCliReviewBackend::new(reviewer.display().to_string());
        let request = ReviewRequest {
            issue: issue(),
            prompt: "Review this prompt.".into(),
            workspace,
            artifact_root,
        };

        let job = backend.start(request).unwrap();
        let job = poll_test_job_until_terminal(&backend, job).unwrap();

        assert_eq!(job.state, ReviewJobState::Completed);
        assert_eq!(
            job.report
                .as_ref()
                .and_then(|report| report.summary.as_deref()),
            Some("Review completed after EOF.")
        );
    }

    #[test]
    fn review_job_ids_are_unique_for_parallel_bursts() {
        let ids = (0..128)
            .map(|_| review_job_id("gemini"))
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(ids.len(), 128);
    }

    #[test]
    fn main_agent_completion_stops_at_agent_review() {
        let decision = review_gate_decision_for_actor(
            &ReviewJob {
                id: "job".into(),
                issue_ref: "#1".into(),
                backend: "fake-reviewer".into(),
                state: ReviewJobState::Completed,
                artifact_path: None,
                ledger_path: None,
                report: Some(AgentReviewReport {
                    reviewer_backend: "fake-reviewer".into(),
                    findings: Vec::new(),
                    summary: None,
                    stdout: None,
                    stderr: None,
                }),
                error: None,
            },
            ReviewActor::MainImplementationAgent,
        );

        assert_eq!(decision.target_state, Some("agent_review"));
        assert!(!matches!(decision.target_state, Some("human_review")));
    }

    #[test]
    fn main_agent_cannot_target_human_review() {
        assert!(!transition_allowed_for_main_agent("human_review"));
        assert!(!transition_allowed_for_main_agent("human review"));
        assert!(transition_allowed_for_main_agent("agent_review"));
        assert!(transition_allowed_for_main_agent("rework"));
    }

    #[test]
    fn review_workpad_names_human_action_boundary() {
        let job = ReviewJob {
            id: "job".into(),
            issue_ref: "#1".into(),
            backend: "fake-reviewer".into(),
            state: ReviewJobState::Completed,
            artifact_path: None,
            ledger_path: None,
            report: Some(AgentReviewReport {
                reviewer_backend: "fake-reviewer".into(),
                findings: Vec::new(),
                summary: None,
                stdout: None,
                stderr: None,
            }),
            error: None,
        };
        let body = render_review_workpad(&issue(), &job);

        assert!(body.contains("Evidence recorded"));
        assert!(body.contains("Review pass evidence: `recorded`"));
        assert!(body.contains("Independent Review Agent may move this issue to Human Review"));
        assert!(body.contains("main implementation agent must not"));
    }

    #[test]
    fn review_job_ledger_record_captures_decision_and_paths() {
        let job = ReviewJob {
            id: "job".into(),
            issue_ref: "#1".into(),
            backend: "fake-reviewer".into(),
            state: ReviewJobState::Completed,
            artifact_path: Some("/tmp/review-artifact.json".into()),
            ledger_path: None,
            report: Some(AgentReviewReport {
                reviewer_backend: "fake-reviewer".into(),
                findings: Vec::new(),
                summary: Some("Review passed.".into()),
                stdout: None,
                stderr: None,
            }),
            error: None,
        };

        let record = review_job_ledger_record(&issue(), &job, "/tmp/review-ledger.json".into());

        assert_eq!(record.issue_ref, "#1");
        assert_eq!(record.job_id, "job");
        assert_eq!(record.worker_key, "review:#1:fake-reviewer");
        assert_eq!(record.decision_outcome, ReviewOutcome::PassedToHumanReview);
        assert_eq!(
            record.decision_target_state.as_deref(),
            Some("human_review")
        );
        assert_eq!(record.summary.as_deref(), Some("Review passed."));
        assert_eq!(record.finding_count, 0);
    }

    #[test]
    fn writes_review_job_ledger_record_json() {
        let temp = tempfile::tempdir().unwrap();
        let job = ReviewJob {
            id: "job".into(),
            issue_ref: "#1".into(),
            backend: "fake-reviewer".into(),
            state: ReviewJobState::Completed,
            artifact_path: None,
            ledger_path: None,
            report: Some(AgentReviewReport {
                reviewer_backend: "fake-reviewer".into(),
                findings: Vec::new(),
                summary: Some("Review passed.".into()),
                stdout: None,
                stderr: None,
            }),
            error: None,
        };

        let path = write_review_job_ledger_record(temp.path(), &issue(), &job).unwrap();
        let body = fs::read_to_string(&path).unwrap();
        let record: ReviewJobLedgerRecord = serde_json::from_str(&body).unwrap();

        assert_eq!(path, temp.path().join("reviews/jobs/_1-job.json"));
        assert_eq!(record.ledger_path, path);
        assert_eq!(
            record.decision_target_state.as_deref(),
            Some("human_review")
        );
    }

    #[test]
    fn review_workpad_includes_ledger_path_when_available() {
        let job = ReviewJob {
            id: "job".into(),
            issue_ref: "#1".into(),
            backend: "fake-reviewer".into(),
            state: ReviewJobState::Completed,
            artifact_path: None,
            ledger_path: Some("/tmp/reviews/jobs/1-job.json".into()),
            report: Some(AgentReviewReport {
                reviewer_backend: "fake-reviewer".into(),
                findings: Vec::new(),
                summary: None,
                stdout: None,
                stderr: None,
            }),
            error: None,
        };

        let body = render_review_workpad(&issue(), &job);

        assert!(body.contains("Review job ledger: /tmp/reviews/jobs/1-job.json"));
    }

    #[test]
    fn review_agent_does_not_set_human_review_for_failed_or_inconclusive_review() {
        let failed = ReviewJob::failed_unavailable("#1", "gemini-cli", "missing executable");
        let failed_decision = review_gate_decision(&failed);
        assert_eq!(failed_decision.outcome, ReviewOutcome::NeedsHumanInput);
        assert_ne!(failed_decision.target_state, Some("human_review"));
        assert!(!transition_allowed_for_review_agent(
            "human_review",
            &failed_decision
        ));

        let inconclusive = ReviewJob {
            id: "job".into(),
            issue_ref: "#1".into(),
            backend: "fake-reviewer".into(),
            state: ReviewJobState::Completed,
            artifact_path: None,
            ledger_path: None,
            report: Some(AgentReviewReport {
                reviewer_backend: "fake-reviewer".into(),
                findings: vec![ReviewFinding {
                    class: ReviewFindingClass::NeedsContext,
                    title: "Need context".into(),
                    body: "Review cannot decide yet.".into(),
                }],
                summary: None,
                stdout: None,
                stderr: None,
            }),
            error: None,
        };
        let inconclusive_decision = review_gate_decision(&inconclusive);
        assert_eq!(
            inconclusive_decision.outcome,
            ReviewOutcome::InconclusiveNeedsRework
        );
        assert_eq!(inconclusive_decision.target_state, Some("rework"));
        assert_ne!(inconclusive_decision.target_state, Some("human_review"));
    }

    #[test]
    fn completed_review_with_missing_workspace_routes_to_rework() {
        let job = completed_gemini_review(
            "I could not complete the review because the workspace is empty.",
        );

        let decision = review_gate_decision(&job);
        let record = review_job_ledger_record(&issue(), &job, "/tmp/review-ledger.json".into());
        let workpad = render_review_workpad(&issue(), &job);

        assert_eq!(decision.outcome, ReviewOutcome::InconclusiveNeedsRework);
        assert_eq!(decision.target_state, Some("rework"));
        assert_eq!(
            record.decision_outcome,
            ReviewOutcome::InconclusiveNeedsRework
        );
        assert_eq!(record.decision_target_state.as_deref(), Some("rework"));
        assert!(workpad.contains("### Inconclusive Review Diagnostic"));
        assert!(workpad.contains("workspace is empty"));
        assert!(!workpad.contains("Review pass evidence: `recorded`"));
    }

    #[test]
    fn completed_review_with_missing_pr_evidence_routes_to_rework() {
        let job = completed_gemini_review(
            "Review could not be completed: expected code changes and PR evidence were missing from the workspace.",
        );

        let decision = review_gate_decision(&job);

        assert_eq!(decision.outcome, ReviewOutcome::InconclusiveNeedsRework);
        assert_eq!(decision.target_state, Some("rework"));
        assert!(transition_allowed_for_review_agent("rework", &decision));
        assert!(!transition_allowed_for_review_agent(
            "human_review",
            &decision
        ));
    }

    #[test]
    fn completed_review_that_cannot_inspect_pr_routes_to_rework() {
        let job = completed_gemini_review(
            "Unable to inspect the PR or required handoff evidence, so this review is inconclusive.",
        );

        let decision = review_gate_decision(&job);

        assert_eq!(decision.outcome, ReviewOutcome::InconclusiveNeedsRework);
        assert_eq!(decision.target_state, Some("rework"));
    }

    #[test]
    fn mechanical_review_freshness_preserves_prior_human_review_evidence() {
        let report = classify_review_freshness(ReviewFreshnessInput {
            issue_ref: "#33".into(),
            prior_head_sha: "old-head".into(),
            current_head_sha: "new-head".into(),
            prior_base_sha: "old-base".into(),
            current_base_sha: "new-base".into(),
            changed_files: vec!["docs/dogfood-readiness.md".into()],
            stale_reason: ReviewStaleReason::MergeConflict,
            rework_class: ReviewReworkClass::MechanicalConflictResolution,
            patch_summary: Some("Resolved merge conflict without semantic changes.".into()),
        });

        assert_eq!(
            report.decision.kind,
            ReviewFreshnessDecisionKind::PriorReviewStillValid
        );
        assert!(report.decision.prior_human_review_valid);
        assert!(!report.decision.human_rereview_required);
        assert_eq!(report.decision.main_agent_target_state, "agent_review");
        assert_eq!(
            report.decision.authorized_next_state.as_deref(),
            Some("merging")
        );
    }

    #[test]
    fn semantic_review_freshness_requires_normal_review_path() {
        let report = classify_review_freshness(ReviewFreshnessInput {
            issue_ref: "#38".into(),
            prior_head_sha: "old-head".into(),
            current_head_sha: "new-head".into(),
            prior_base_sha: "same-base".into(),
            current_base_sha: "same-base".into(),
            changed_files: vec!["src/review.rs".into()],
            stale_reason: ReviewStaleReason::ReviewOutdated,
            rework_class: ReviewReworkClass::SemanticChange,
            patch_summary: Some("Changed review decision behavior.".into()),
        });

        assert_eq!(
            report.decision.kind,
            ReviewFreshnessDecisionKind::PriorReviewInvalidated
        );
        assert!(!report.decision.prior_human_review_valid);
        assert!(report.decision.human_rereview_required);
        assert_eq!(report.decision.main_agent_target_state, "agent_review");
        assert_eq!(
            report.decision.authorized_next_state.as_deref(),
            Some("agent_review")
        );
    }

    #[test]
    fn review_freshness_workpad_records_authority_boundary() {
        let report = classify_review_freshness(ReviewFreshnessInput {
            issue_ref: "#33".into(),
            prior_head_sha: "old-head".into(),
            current_head_sha: "new-head".into(),
            prior_base_sha: "old-base".into(),
            current_base_sha: "new-base".into(),
            changed_files: Vec::new(),
            stale_reason: ReviewStaleReason::BaseBranchUpdated,
            rework_class: ReviewReworkClass::BaseRefresh,
            patch_summary: None,
        });
        let workpad = render_review_freshness_workpad(&report);

        assert!(workpad.contains("Prior Human Review still valid: `true`"));
        assert!(workpad.contains("Main implementation agent still stops at `Agent Review`"));
        assert!(workpad.contains("Human Review"));
    }

    #[test]
    fn review_run_eligibility_accepts_agent_review_issue() {
        let mut issue = issue();
        issue
            .linked_pull_requests
            .push(crate::model::LinkedPullRequest {
                url: Some("https://github.com/Alive24/jade-symphony/pull/1".into()),
                state: Some("OPEN".into()),
                is_draft: Some(false),
                ..Default::default()
            });
        assert_eq!(
            review_run_eligibility(&issue, "Agent Review", "fake"),
            ReviewRunEligibility::Eligible {
                worker_key: "review:#1:fake".into()
            }
        );
    }

    #[test]
    fn review_run_eligibility_rejects_non_agent_review_issue() {
        let mut issue = issue();
        issue.state = "Todo".into();

        assert_eq!(
            review_run_eligibility(&issue, "Agent Review", "fake"),
            ReviewRunEligibility::NotInAgentReview {
                current_state: "Todo".into()
            }
        );
    }

    #[test]
    fn review_run_eligibility_rejects_agent_review_missing_pr_linkage() {
        let mut issue = issue();
        issue.linked_pull_requests.clear();

        assert!(matches!(
            review_run_eligibility(&issue, "Agent Review", "fake"),
            ReviewRunEligibility::InvalidHandoff { reason }
                if reason.contains("no verified Project-visible linked PR")
        ));
    }

    #[test]
    fn review_run_eligibility_rejects_agent_review_draft_pr() {
        let mut issue = issue();
        issue
            .linked_pull_requests
            .push(crate::model::LinkedPullRequest {
                url: Some("https://github.com/Alive24/jade-symphony/pull/1".into()),
                state: Some("OPEN".into()),
                is_draft: Some(true),
                ..Default::default()
            });

        assert!(matches!(
            review_run_eligibility(&issue, "Agent Review", "fake"),
            ReviewRunEligibility::InvalidHandoff { reason }
                if reason.contains("linked draft PR")
        ));
    }

    #[test]
    fn review_run_eligibility_detects_existing_worker_marker() {
        let mut issue = issue();
        issue.project_fields.insert(
            "Review Worker".into(),
            serde_json::Value::String("queued review:#1:fake".into()),
        );

        assert_eq!(
            review_run_eligibility(&issue, "Agent Review", "fake"),
            ReviewRunEligibility::AlreadyQueued {
                worker_key: "review:#1:fake".into()
            }
        );
    }

    #[test]
    fn review_run_eligibility_detects_gemini_single_select_claim() {
        let mut issue = issue();
        issue.project_fields.insert(
            "Review Agent".into(),
            serde_json::Value::String("Gemini A".into()),
        );

        assert_eq!(
            review_run_eligibility(&issue, "Agent Review", "gemini-cli"),
            ReviewRunEligibility::AlreadyQueued {
                worker_key: "review:#1:gemini-cli".into()
            }
        );
    }

    #[test]
    fn review_run_eligibility_detects_workpad_worker_marker() {
        let mut issue = issue();
        issue.description =
            Some("## Workpad\n\nReview worker running with key `review:#1:fake`.".into());

        assert_eq!(
            review_run_eligibility(&issue, "Agent Review", "fake"),
            ReviewRunEligibility::AlreadyQueued {
                worker_key: "review:#1:fake".into()
            }
        );
    }

    #[test]
    fn review_run_eligibility_ignores_terminal_failed_workpad_for_retry() {
        let mut issue = issue();
        issue.project_fields.insert(
            "Review Agent".into(),
            serde_json::Value::String("Gemini A".into()),
        );
        let job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "Gemini review command exited with status 1: workspace is not trusted",
        );
        issue.description = Some(render_review_workpad(&issue, &job));

        assert_eq!(
            review_run_eligibility(&issue, "Agent Review", "gemini-cli"),
            ReviewRunEligibility::Eligible {
                worker_key: "review:#1:gemini-cli".into()
            }
        );
    }

    #[test]
    fn review_run_eligibility_ignores_failed_workpad_with_running_error_text() {
        let mut issue = issue();
        let job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "Gemini review command exited with status 1: CLI is not running in a trusted directory",
        );
        issue.description = Some(render_review_workpad(&issue, &job));

        assert_eq!(
            review_run_eligibility(&issue, "Agent Review", "gemini-cli"),
            ReviewRunEligibility::Eligible {
                worker_key: "review:#1:gemini-cli".into()
            }
        );
    }

    #[test]
    fn review_workpad_surfaces_usage_limit_without_human_review() {
        let job = ReviewJob {
            id: "job".into(),
            issue_ref: "#1".into(),
            backend: "gemini-cli".into(),
            state: ReviewJobState::Failed,
            artifact_path: None,
            ledger_path: None,
            report: None,
            error: Some("rate limit exceeded; retry later".into()),
        };
        let workpad = render_review_workpad(&issue(), &job);

        assert_eq!(
            review_usage_limit_pause(&job).unwrap().classifier,
            "rate_limit"
        );
        assert!(workpad.contains("Usage-limit classifier: `rate_limit`"));
        assert!(workpad.contains("must not move to Human Review"));
    }
}
