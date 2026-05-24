use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
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
use crate::model::{
    is_native_subissue, native_subissue_human_review_exception, normalize_state, TrackerIssue,
};
use crate::workspace::safe_identifier;

mod freshness;
mod gemini_health;

pub use freshness::{
    classify_review_freshness, render_review_freshness_workpad, ReviewFreshnessDecision,
    ReviewFreshnessDecisionKind, ReviewFreshnessInput, ReviewFreshnessReport, ReviewReworkClass,
    ReviewStaleReason,
};
pub use gemini_health::{
    gemini_prelaunch_health_diagnostic, gemini_review_health_diagnostic, review_failure_signature,
    GeminiReviewHealthCategory, GeminiReviewHealthDiagnostic, GeminiReviewRecoveryPolicy,
};

use gemini_health::diagnose_gemini_spawn_failure;

const AGENT_REVIEW_WORKPAD_TEMPLATE: &str =
    include_str!("../workflows/template/workpad/agent-review.md");
const LOG_BLOCK_LIMIT: usize = 2_000;

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
    pub exit_status: Option<String>,
    pub session_id: Option<String>,
}

impl AgentReviewReport {
    pub fn blocks_progress(&self) -> bool {
        self.findings.iter().any(review_finding_blocks_progress)
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

fn review_finding_blocks_progress(finding: &ReviewFinding) -> bool {
    finding.class == ReviewFindingClass::Confirmed && !human_owned_uat_finding(finding)
}

fn human_owned_uat_finding(finding: &ReviewFinding) -> bool {
    let text = format!("{} {}", finding.title, finding.body).to_ascii_lowercase();
    if !text.contains("uat") {
        return false;
    }

    let missing_uat = text.contains("missing uat")
        || text.contains("uat was not run")
        || text.contains("uat has not been run")
        || text.contains("uat was skipped")
        || text.contains("uat not run")
        || text.contains("live uat");
    if !missing_uat {
        return false;
    }

    let implementation_deliverable = [
        "uat harness",
        "uat fixture",
        "controlled rehearsal",
        "rehearsal path",
        "dogfood workflow",
        "workflow capability",
        "implemented",
        "implementing",
        "implementation deliverable",
    ]
    .iter()
    .any(|pattern| text.contains(pattern));

    !implementation_deliverable
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewOutcome {
    PassedToHumanReview,
    PassedToMerging,
    NeedsRework,
    InconclusiveNeedsRework,
    NeedsHumanInput,
    BackendUnavailable,
    StillRunning,
    Cancelled,
}

impl ReviewOutcome {
    pub fn is_passed(self) -> bool {
        matches!(self, Self::PassedToHumanReview | Self::PassedToMerging)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewGateDecision {
    pub outcome: ReviewOutcome,
    pub target_state: Option<&'static str>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRepeatedFailureEvidence {
    pub repeat_count: usize,
    pub first_job_id: String,
    pub previous_job_id: String,
    pub signature: String,
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
                    exit_status: None,
                    session_id: None,
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
                    exit_status: None,
                    session_id: None,
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
    model: Option<String>,
    allowed_tools: Vec<String>,
    children: Arc<Mutex<BTreeMap<String, Child>>>,
}

impl GeminiCliReviewBackend {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            model: None,
            allowed_tools: Vec::new(),
            children: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn with_headless_options(
        command: impl Into<String>,
        model: Option<String>,
        allowed_tools: Vec<String>,
    ) -> Self {
        Self {
            command: command.into(),
            model,
            allowed_tools,
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

        let args = gemini_cli_headless_args(self.model.as_deref(), &self.allowed_tools);
        let mut child = Command::new(&self.command)
            .args(&args)
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
        let exit_status = output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "terminated by signal".into());
        let parsed_output = parse_gemini_stdout(&stdout);
        let output_artifact = write_gemini_review_artifact(
            job.artifact_path.as_deref(),
            &job.id,
            &stdout,
            &stderr,
            &exit_status,
            parsed_output.session_id.as_deref(),
            &parsed_output.response,
        )?;
        job.artifact_path = Some(output_artifact);

        if output.status.success() {
            let findings = classify_findings(&parsed_output.response);
            job.state = ReviewJobState::Completed;
            job.report = Some(AgentReviewReport {
                reviewer_backend: self.kind().into(),
                findings,
                summary: Some(
                    first_non_empty_line(&parsed_output.response)
                        .unwrap_or("Review completed.")
                        .into(),
                ),
                stdout: Some(stdout),
                stderr: Some(stderr),
                exit_status: Some(exit_status),
                session_id: parsed_output.session_id,
            });
        } else {
            job.state = ReviewJobState::Failed;
            let detail = stderr.trim();
            job.error = Some(if detail.is_empty() {
                format!("Gemini review command exited with status {exit_status} and no stderr.")
            } else {
                format!("Gemini review command exited with status {exit_status}: {detail}")
            });
            job.report = Some(AgentReviewReport {
                reviewer_backend: self.kind().into(),
                findings: Vec::new(),
                summary: first_non_empty_line(&parsed_output.response).map(str::to_string),
                stdout: Some(stdout),
                stderr: Some(stderr),
                exit_status: Some(exit_status),
                session_id: parsed_output.session_id,
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

pub fn gemini_cli_headless_args(model: Option<&str>, allowed_tools: &[String]) -> Vec<String> {
    let mut args = vec![
        "--skip-trust".to_string(),
        "--prompt".to_string(),
        String::new(),
        "--output-format".to_string(),
        "json".to_string(),
    ];
    if let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    if !allowed_tools.is_empty() {
        args.extend(["--allowed-tools".to_string(), allowed_tools.join(",")]);
    }
    args
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedGeminiStdout {
    response: String,
    session_id: Option<String>,
}

fn parse_gemini_stdout(stdout: &str) -> ParsedGeminiStdout {
    extract_gemini_json_envelope(stdout)
        .and_then(|value| {
            let response = value
                .get("response")
                .and_then(|response| response.as_str())
                .map(str::to_string)?;
            let session_id = value
                .get("session_id")
                .and_then(|session_id| session_id.as_str())
                .map(str::to_string);
            Some(ParsedGeminiStdout {
                response,
                session_id,
            })
        })
        .unwrap_or_else(|| ParsedGeminiStdout {
            response: stdout.to_string(),
            session_id: None,
        })
}

fn extract_gemini_json_envelope(stdout: &str) -> Option<serde_json::Value> {
    for (start, ch) in stdout.char_indices() {
        if ch != '{' {
            continue;
        }
        let mut deserializer = serde_json::Deserializer::from_str(&stdout[start..]);
        if let Ok(value) = serde_json::Value::deserialize(&mut deserializer) {
            return Some(value);
        }
    }
    None
}

fn write_gemini_review_artifact(
    prompt_artifact_path: Option<&Path>,
    job_id: &str,
    stdout: &str,
    stderr: &str,
    exit_status: &str,
    session_id: Option<&str>,
    response: &str,
) -> Result<PathBuf, ReviewError> {
    let artifact_root = prompt_artifact_path
        .and_then(Path::parent)
        .ok_or_else(|| ReviewError::Artifact("missing Gemini prompt artifact path".into()))?;
    let output_path = artifact_root.join(format!("{job_id}.output.json"));
    let artifact = serde_json::json!({
        "job_id": job_id,
        "prompt_artifact_path": prompt_artifact_path.map(|path| path.display().to_string()),
        "stdout": stdout,
        "stderr": stderr,
        "exit_status": exit_status,
        "session_id": session_id,
        "response": response,
    });
    fs::write(
        &output_path,
        serde_json::to_string_pretty(&artifact)
            .map_err(|error| ReviewError::Artifact(error.to_string()))?,
    )
    .map_err(|error| ReviewError::Artifact(error.to_string()))?;
    Ok(output_path)
}

pub fn classify_findings(output: &str) -> Vec<ReviewFinding> {
    let result = parse_review_result(output);
    let mut findings = output
        .lines()
        .filter_map(|line| {
            parse_finding_line(line).or_else(|| {
                matches!(
                    result,
                    Some(ParsedReviewResult::Rework) | Some(ParsedReviewResult::NeedsContext)
                )
                .then(|| parse_loose_finding_line(line))
                .flatten()
            })
        })
        .collect::<Vec<_>>();

    match result {
        Some(ParsedReviewResult::Rework)
            if !findings
                .iter()
                .any(|finding| finding.class == ReviewFindingClass::Confirmed) =>
        {
            findings.push(synthetic_review_result_finding(
                ReviewFindingClass::Confirmed,
                "Review result requires rework",
                output,
            ));
        }
        Some(ParsedReviewResult::NeedsContext)
            if !findings
                .iter()
                .any(|finding| finding.class == ReviewFindingClass::NeedsContext) =>
        {
            findings.push(synthetic_review_result_finding(
                ReviewFindingClass::NeedsContext,
                "Review result needs context",
                output,
            ));
        }
        _ => {}
    }

    findings
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

pub fn review_gate_decision_for_issue(job: &ReviewJob, issue: &TrackerIssue) -> ReviewGateDecision {
    let decision = review_gate_decision(job);
    if decision.outcome == ReviewOutcome::PassedToHumanReview
        && review_pass_target_state(issue) == "merging"
    {
        return ReviewGateDecision {
            outcome: ReviewOutcome::PassedToMerging,
            target_state: Some("merging"),
            message: "Independent Agent Review passed with recorded evidence; native subissue routes directly to Merging because the parent issue owns final Human Review and UAT.".into(),
        };
    }
    decision
}

pub fn review_pass_target_state(issue: &TrackerIssue) -> &'static str {
    if is_native_subissue(issue) && !native_subissue_human_review_exception(issue) {
        "merging"
    } else {
        "human_review"
    }
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
        ReviewJobState::Failed | ReviewJobState::TimedOut => {
            if let Some(diagnostic) = gemini_review_health_diagnostic(job) {
                if diagnostic.is_recoverable() {
                    return ReviewGateDecision {
                        outcome: ReviewOutcome::BackendUnavailable,
                        target_state: Some("agent_review"),
                        message: format!(
                            "Gemini review backend is {}; issue remains in Agent Review for {}.",
                            diagnostic.category.as_str(),
                            diagnostic.recovery_policy.as_str()
                        ),
                    };
                }

                return ReviewGateDecision {
                    outcome: ReviewOutcome::NeedsHumanInput,
                    target_state: Some("need_human_input"),
                    message: format!(
                        "Gemini review backend is blocked by {}; human input is required.",
                        diagnostic.category.as_str()
                    ),
                };
            }

            if review_required_operator_actions(job).is_some() {
                return ReviewGateDecision {
                    outcome: ReviewOutcome::BackendUnavailable,
                    target_state: Some("agent_review"),
                    message:
                        "Agent Review backend is blocked by required operator action; issue remains in Agent Review."
                            .into(),
                };
            }

            ReviewGateDecision {
                outcome: ReviewOutcome::NeedsHumanInput,
                target_state: Some("need_human_input"),
                message: "Agent review failed or timed out; human input is required.".into(),
            }
        }
        ReviewJobState::Cancelled => ReviewGateDecision {
            outcome: ReviewOutcome::Cancelled,
            target_state: Some("agent_review"),
            message: "Agent review was cancelled; issue remains in Agent Review.".into(),
        },
    }
}

pub fn render_repeated_review_failure_workpad(
    issue: &TrackerIssue,
    job: &ReviewJob,
    repeat: &ReviewRepeatedFailureEvidence,
) -> String {
    let decision = review_gate_decision_for_actor(job, ReviewActor::IndependentReviewAgent);
    let diagnostic = gemini_review_health_diagnostic(job);
    let mut lines = vec![
        "## Jade Symphony Agent Review Run".to_string(),
        String::new(),
        "### Repeated Backend Failure".into(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Worker key: `{}`", review_worker_key(issue, &job.backend)),
        format!("- Reviewer backend: `{}`", job.backend),
        format!("- Job state: `{:?}`", job.state),
        format!("- Current job id: `{}`", job.id),
        format!("- First same-cause job id: `{}`", repeat.first_job_id),
        format!("- Previous same-cause job id: `{}`", repeat.previous_job_id),
        format!("- Same-cause repeat count: `{}`", repeat.repeat_count),
        format!("- Failure signature: `{}`", repeat.signature),
        format!("- Decision: {}", decision.message),
        format!(
            "- Target state after review routing: `{}`",
            decision.target_state.unwrap_or("none")
        ),
        "- Evidence policy: compact repeat line only; full diagnostic was already recorded for the first same-cause attempt.".into(),
    ];

    if let Some(path) = job.ledger_path.as_ref() {
        lines.push(format!("- Review job ledger: `{}`", path.display()));
    }

    if let Some(diagnostic) = diagnostic {
        lines.push(format!(
            "- Gemini health: `{}` / `{}`",
            diagnostic.category.as_str(),
            diagnostic.recovery_policy.as_str()
        ));
        lines.push(format!("- Operator status: {}", diagnostic.operator_status));
        if let Some(retry_after_ms) = diagnostic.retry_after_ms {
            lines.push(format!("- Retry-after: `{retry_after_ms}ms`"));
        }
    }

    lines.join("\n")
}

pub fn render_gemini_health_section(job: &ReviewJob) -> String {
    let Some(diagnostic) = gemini_review_health_diagnostic(job) else {
        return String::new();
    };

    let retry_after = diagnostic
        .retry_after_ms
        .map(|value| format!("- Retry-after: `{value}ms`"))
        .unwrap_or_else(|| "- Retry-after: `not detected`".into());

    render_section(
        "Gemini Backend Health",
        &[
            format!("- Classification: `{}`", diagnostic.category.as_str()),
            format!("- Reason: `{}`", diagnostic.reason_code),
            format!(
                "- Recovery policy: `{}`",
                diagnostic.recovery_policy.as_str()
            ),
            format!("- Status: {}", diagnostic.operator_status),
            retry_after,
            format!("- Diagnostic: {}", diagnostic.message),
        ]
        .join("\n"),
    )
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
            || value.contains("job state: `failed`")
            || value.contains("job state: timedout")
            || value.contains("job state: `timedout`")
            || value.contains("job state: timed out")
            || value.contains("job state: `timed out`")
            || value.contains("job state: cancelled")
            || value.contains("job state: `cancelled`"))
        && (value.contains("required operator action")
            || value.contains("review backend")
            || value.contains("repeated backend failure")
            || value.contains("gemini backend health")
            || value.contains("gemini review command")
            || value.contains("retry: rerun `review loop`")
            || value.contains("retry: rerun review loop"))
}

pub fn render_review_workpad(issue: &TrackerIssue, job: &ReviewJob) -> String {
    let decision = review_gate_decision_for_actor(job, ReviewActor::IndependentReviewAgent);
    let mut attempt_details = Vec::new();
    if let Some(error) = &job.error {
        attempt_details.push(render_attempt_error(error));
    }
    if let Some(report) = &job.report {
        if let Some(status) = report.exit_status.as_deref() {
            attempt_details.push(format!("- Exit status: `{status}`"));
        }
        if let Some(session_id) = report.session_id.as_deref() {
            attempt_details.push(format!("- Gemini session id: `{session_id}`"));
        }
    }
    if attempt_details.is_empty() {
        attempt_details.push("- No attempt details captured yet.".into());
    }

    let operator_action_section = if let Some(actions) = review_required_operator_actions(job) {
        render_section("Required Operator Action", &actions.join("\n"))
    } else {
        String::new()
    };
    let gemini_health_section = render_gemini_health_section(job);

    let usage_limit_section = review_usage_limit_pause(job)
        .map(|pause| {
            render_section(
                "Usage Limit Diagnostic",
                &[
                    format!("- Usage-limit classifier: `{}`", pause.classifier),
                    format!("- Usage-limit evidence: {}", pause.evidence),
                    "- Review did not pass; unavailable or inconclusive review must not move to Human Review.".into(),
                ]
                .join("\n"),
            )
        })
        .unwrap_or_default();

    let inconclusive_section = job
        .report
        .as_ref()
        .and_then(|report| report.inconclusive_reason())
        .map(|reason| {
            render_section(
                "Inconclusive Review Diagnostic",
                &[
                    format!("- Reason: {reason}"),
                    "- Automatic Review Agent output did not establish a conclusive pass.".into(),
                    "- Route to `Rework`; do not move to `Human Review`.".into(),
                ]
                .join("\n"),
            )
        })
        .unwrap_or_default();

    let agent_review_note = job
        .report
        .as_ref()
        .and_then(agent_review_note)
        .unwrap_or_else(|| "- No Agent Review note captured.".into());
    let has_agent_review_note = agent_review_note != "- No Agent Review note captured.";
    let findings_section =
        render_parsed_findings_section(job.report.as_ref(), has_agent_review_note);

    let stdout_section = job
        .report
        .as_ref()
        .and_then(|report| render_log_section("Stdout", report.stdout.as_deref()))
        .unwrap_or_default();
    let stderr_section = job
        .report
        .as_ref()
        .and_then(|report| render_log_section("Stderr", report.stderr.as_deref()))
        .unwrap_or_default();

    let pass_evidence_section = if decision.outcome.is_passed() {
        let routing_note = if decision.outcome == ReviewOutcome::PassedToMerging {
            "- Review pass evidence: `recorded`\nEvidence recorded. Independent Review Agent may move this native subissue directly to Merging; final Human Review and UAT remain owned by the parent issue."
        } else {
            "- Review pass evidence: `recorded`\nEvidence recorded. Independent Review Agent may move this issue to Human Review; the main implementation agent must not."
        };
        render_section("Review Pass Evidence", routing_note)
    } else {
        String::new()
    };

    render_template(
        AGENT_REVIEW_WORKPAD_TEMPLATE,
        &[
            ("generated_at", current_gmt_timestamp()),
            ("issue_ref", issue.identifier.clone()),
            ("issue_title", issue.title.clone()),
            ("worker_key", review_worker_key(issue, &job.backend)),
            ("reviewer_backend", job.backend.clone()),
            ("run_id", job.id.clone()),
            ("job_state", format!("{:?}", job.state)),
            ("decision", decision.message),
            (
                "target_state",
                decision.target_state.unwrap_or("none").to_string(),
            ),
            ("result", format!("{:?}", decision.outcome)),
            ("pr_line", review_pr_line(issue)),
            (
                "artifact_line",
                job.artifact_path
                    .as_ref()
                    .map(|path| format!("- Artifact: `{}`", path.display()))
                    .unwrap_or_else(|| "- Artifact: `not recorded`".into()),
            ),
            (
                "ledger_line",
                job.ledger_path
                    .as_ref()
                    .map(|path| format!("- Review job ledger: `{}`", path.display()))
                    .unwrap_or_else(|| "- Review job ledger: `not recorded`".into()),
            ),
            ("evidence_summary", review_evidence_summary(job)),
            ("job_id", job.id.clone()),
            ("attempt_details", attempt_details.join("\n")),
            ("operator_action_section", operator_action_section),
            ("gemini_health_section", gemini_health_section),
            ("usage_limit_section", usage_limit_section),
            ("inconclusive_section", inconclusive_section),
            ("agent_review_note", agent_review_note),
            ("findings_section", findings_section),
            ("stdout_section", stdout_section),
            ("stderr_section", stderr_section),
            ("pass_evidence_section", pass_evidence_section),
        ],
    )
}

fn review_pr_line(issue: &TrackerIssue) -> String {
    let pr = issue
        .linked_pull_requests
        .iter()
        .find_map(
            |pull_request| match (pull_request.number, pull_request.url.as_deref()) {
                (Some(number), Some(url)) => Some(format!("#{number} {url}")),
                (Some(number), None) => Some(format!("#{number}")),
                (None, Some(url)) => Some(url.to_string()),
                (None, None) => None,
            },
        )
        .unwrap_or_else(|| "not recorded".into());
    format!("- PR: `{pr}`")
}

fn review_evidence_summary(job: &ReviewJob) -> String {
    let finding_count = job
        .report
        .as_ref()
        .map(|report| report.findings.len())
        .unwrap_or_default();
    let artifact = if job.artifact_path.is_some() {
        "artifact recorded"
    } else {
        "artifact not recorded"
    };
    let ledger = if job.ledger_path.is_some() {
        "ledger recorded"
    } else {
        "ledger not recorded"
    };
    format!("{finding_count} parsed finding(s); {artifact}; {ledger}.")
}

fn render_parsed_findings_section(
    report: Option<&AgentReviewReport>,
    has_agent_review_note: bool,
) -> String {
    if has_agent_review_note {
        return String::new();
    }

    let findings = match report {
        Some(report) if report.findings.is_empty() => {
            "- No confirmed, plausible, rejected, or needs-context findings.".into()
        }
        Some(report) => report
            .findings
            .iter()
            .map(|finding| {
                format!(
                    "- {:?}: {} - {}",
                    finding.class, finding.title, finding.body
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        None => "- No report captured yet.".into(),
    };

    render_section("Findings", &findings)
}

fn render_template(template: &str, replacements: &[(&str, String)]) -> String {
    let mut rendered = template.to_string();
    for (key, value) in replacements {
        rendered = rendered.replace(&format!("{{{{{key}}}}}"), value);
    }
    rendered.trim_end().to_string()
}

fn render_section(title: &str, body: &str) -> String {
    if body.trim().is_empty() {
        return String::new();
    }
    format!("### {title}\n\n{}\n\n", body.trim())
}

fn render_log_section(title: &str, content: Option<&str>) -> Option<String> {
    let content = content?.trim();
    if content.is_empty() {
        return None;
    }
    Some(render_section(
        title,
        &format!(
            "<details>\n<summary>{title}</summary>\n\n```text\n{}\n```\n\n</details>",
            truncate_log(content)
        ),
    ))
}

fn render_attempt_error(error: &str) -> String {
    let summary = error
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or("review backend error");
    format!(
        "<details>\n<summary>Error: {}</summary>\n\n```text\n{}\n```\n\n</details>",
        html_escape_summary(&truncate_summary(summary, 180)),
        truncate_log(error)
    )
}

fn truncate_summary(summary: &str, limit: usize) -> String {
    if summary.len() <= limit {
        return summary.to_string();
    }

    let mut end = limit;
    while !summary.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &summary[..end])
}

fn html_escape_summary(summary: &str) -> String {
    summary
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn agent_review_note(report: &AgentReviewReport) -> Option<String> {
    if let Some(stdout) = report.stdout.as_deref() {
        if let Some(value) = extract_gemini_json_envelope(stdout) {
            if let Some(note) = value
                .get("response")
                .or_else(|| value.get("note"))
                .and_then(|response| response.as_str())
                .map(str::trim)
                .filter(|response| !response.is_empty())
            {
                return Some(normalize_agent_review_note_headings(note));
            }
        }
    }

    report
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(normalize_agent_review_note_headings)
}

fn normalize_agent_review_note_headings(note: &str) -> String {
    note.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let indent_len = line.len() - trimmed.len();
            let heading_len = trimmed.chars().take_while(|ch| *ch == '#').count();
            if (1..=6).contains(&heading_len)
                && trimmed
                    .chars()
                    .nth(heading_len)
                    .is_some_and(char::is_whitespace)
            {
                if heading_len < 4 {
                    let title = trimmed[heading_len..].trim();
                    format!("{}#### {title}", &line[..indent_len])
                } else {
                    line.to_string()
                }
            } else if let Some(title) = agent_review_note_label_heading(trimmed) {
                format!("{}#### {title}", &line[..indent_len])
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn agent_review_note_label_heading(line: &str) -> Option<&str> {
    let title = line.strip_suffix(':').unwrap_or(line).trim();
    let prefix = title
        .split_once(':')
        .map_or(title, |(prefix, _)| prefix.trim());
    matches!(
        prefix.to_ascii_lowercase().as_str(),
        "review result" | "evidence" | "findings" | "verification" | "checklist review"
    )
    .then_some(title)
}

fn truncate_log(content: &str) -> String {
    if content.len() <= LOG_BLOCK_LIMIT {
        return content.to_string();
    }

    let mut end = LOG_BLOCK_LIMIT;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} [... truncated]", &content[..end])
}

fn current_gmt_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format_gmt_timestamp(seconds)
}

fn format_gmt_timestamp(seconds_since_unix_epoch: u64) -> String {
    let days = (seconds_since_unix_epoch / 86_400) as i64;
    let seconds_of_day = seconds_since_unix_epoch % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} GMT")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_unix_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

fn review_required_operator_actions(job: &ReviewJob) -> Option<Vec<String>> {
    let error = job.error.as_deref().unwrap_or_default();
    if let Some(diagnostic) = gemini_review_health_diagnostic(job) {
        if diagnostic.is_recoverable() {
            return None;
        }

        return Some(vec![
            format!("- Gemini backend health: `{}`.", diagnostic.category.as_str()),
            format!("- Reason: `{}`.", diagnostic.reason_code),
            format!("- Status: {}", diagnostic.operator_status),
            "- This issue must not move to `Human Review` until an independent Review Agent records passing review evidence.".into(),
            "- Human intervention is required before automatic review can continue.".into(),
        ]);
    }

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
        "- For Gemini CLI, prefer an absolute `review_lane.gemini_command` path or export a worker PATH that resolves `gemini`.".into(),
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

pub fn transition_allowed_for_main_agent(normalized_state: &str) -> bool {
    !matches!(normalized_state, "human_review" | "human review")
}

pub fn transition_allowed_for_review_agent(
    normalized_state: &str,
    decision: &ReviewGateDecision,
) -> bool {
    match normalized_state {
        "human_review" | "human review" => decision.outcome == ReviewOutcome::PassedToHumanReview,
        "merging" => decision.outcome == ReviewOutcome::PassedToMerging,
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
                ReviewOutcome::BackendUnavailable
                    | ReviewOutcome::StillRunning
                    | ReviewOutcome::Cancelled
            )
        }
        _ => true,
    }
}

fn parse_finding_line(line: &str) -> Option<ReviewFinding> {
    let trimmed = trim_finding_list_marker(line);
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
    let (title, body) = rest.split_once(':')?;
    if title.trim().is_empty() || body.trim().is_empty() {
        return None;
    }
    Some(ReviewFinding {
        class,
        title: title.trim().to_string(),
        body: body.trim().to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedReviewResult {
    Pass,
    Rework,
    NeedsContext,
}

fn parse_review_result(output: &str) -> Option<ParsedReviewResult> {
    output.lines().find_map(|line| {
        let normalized = line
            .trim()
            .trim_matches(|ch: char| ch == '*' || ch == '_' || ch == '`')
            .to_ascii_lowercase();
        let (_, value) = normalized.split_once("review result:")?;
        let value = value.trim();
        if value.starts_with("pass") {
            Some(ParsedReviewResult::Pass)
        } else if value.starts_with("rework") {
            Some(ParsedReviewResult::Rework)
        } else if value.starts_with("needs_context")
            || value.starts_with("needs context")
            || value.starts_with("need context")
        {
            Some(ParsedReviewResult::NeedsContext)
        } else {
            None
        }
    })
}

fn parse_loose_finding_line(line: &str) -> Option<ReviewFinding> {
    let trimmed = trim_finding_list_marker(line);
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
    if rest.is_empty() {
        return None;
    }
    Some(ReviewFinding {
        class,
        title: summarize_finding_title(rest),
        body: rest.to_string(),
    })
}

fn trim_finding_list_marker(line: &str) -> &str {
    let trimmed = line.trim();
    trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .unwrap_or(trimmed)
        .trim_start()
}

fn synthetic_review_result_finding(
    class: ReviewFindingClass,
    title: &str,
    output: &str,
) -> ReviewFinding {
    ReviewFinding {
        class,
        title: title.into(),
        body: first_review_result_body_line(output)
            .unwrap_or("Review backend returned this routing result without a parseable finding.")
            .into(),
    }
}

fn first_review_result_body_line(output: &str) -> Option<&str> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find(|line| !line.to_ascii_lowercase().contains("review result:"))
}

fn summarize_finding_title(text: &str) -> String {
    let mut title = text
        .split(['.', ';', '\n'])
        .next()
        .unwrap_or(text)
        .trim()
        .to_string();
    const MAX_TITLE_CHARS: usize = 96;
    if title.chars().count() > MAX_TITLE_CHARS {
        title = title.chars().take(MAX_TITLE_CHARS).collect::<String>();
        title.push_str("...");
    }
    title
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

    fn native_subissue(mut issue: TrackerIssue) -> TrackerIssue {
        issue.identifier = "#272".into();
        issue.project_fields.insert(
            "GitHub Native Parent".into(),
            serde_json::json!({"identifier": "#243", "state": "OPEN"}),
        );
        issue
            .project_fields
            .insert("Native Parent Issue".into(), serde_json::json!("#243"));
        issue
    }

    fn parent_issue(mut issue: TrackerIssue) -> TrackerIssue {
        issue.identifier = "#243".into();
        issue.project_fields.insert(
            "GitHub Native Subissues".into(),
            serde_json::json!([
                {"identifier": "#272", "project_state": "Done"}
            ]),
        );
        issue
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
                exit_status: None,
                session_id: None,
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
                    exit_status: None,
                    session_id: None,
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
    fn gemini_headless_args_include_prompt_json_model_and_allowed_tools() {
        let args = gemini_cli_headless_args(
            Some("gemini-3.1-pro-preview"),
            &["run_shell_command".into()],
        );

        assert_eq!(
            args,
            vec![
                "--skip-trust",
                "--prompt",
                "",
                "--output-format",
                "json",
                "--model",
                "gemini-3.1-pro-preview",
                "--allowed-tools",
                "run_shell_command",
            ]
        );
    }

    #[test]
    fn gemini_backend_uses_headless_args_stdin_prompt_and_json_response() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("review-workspace");
        let artifact_root = temp.path().join("reviews");
        let reviewer = temp.path().join("reviewer.sh");
        fs::write(
            &reviewer,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > args.txt\ncat > prompt.txt\nprintf 'warning prelude\\n{\"session_id\":\"gemini-session-7\",\"response\":\"Review completed.\\\\n[Rejected] Noise: not a bug\"}\\n'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&reviewer).unwrap().permissions();
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&reviewer, permissions).unwrap();

        let backend = GeminiCliReviewBackend::with_headless_options(
            reviewer.display().to_string(),
            Some("gemini-3.1-pro-preview".into()),
            vec!["run_shell_command".into()],
        );
        let request = ReviewRequest {
            issue: issue(),
            prompt: "Review this prompt.".into(),
            workspace: workspace.clone(),
            artifact_root: artifact_root.clone(),
        };

        let job = backend.start(request).unwrap();
        let job = poll_review_job_until_terminal(
            &backend,
            job,
            Duration::from_secs(5),
            Duration::from_millis(10),
        )
        .unwrap();

        assert_eq!(job.state, ReviewJobState::Completed);
        assert_eq!(
            fs::read_to_string(workspace.join("args.txt")).unwrap(),
            "--skip-trust\n--prompt\n\n--output-format\njson\n--model\ngemini-3.1-pro-preview\n--allowed-tools\nrun_shell_command\n"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("prompt.txt")).unwrap(),
            "Review this prompt."
        );
        let report = job.report.as_ref().unwrap();
        assert_eq!(report.summary.as_deref(), Some("Review completed."));
        assert_eq!(report.session_id.as_deref(), Some("gemini-session-7"));
        assert_eq!(report.exit_status.as_deref(), Some("0"));
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].class, ReviewFindingClass::Rejected);
        assert!(job
            .artifact_path
            .as_ref()
            .and_then(|path| path.file_name())
            .is_some_and(|name| name.to_string_lossy() == format!("{}.output.json", job.id)));
    }

    #[test]
    fn parses_gemini_warning_prelude_plus_json_envelope() {
        let parsed = parse_gemini_stdout(
            "Using experimental output\n{\"session_id\":\"abc\",\"response\":\"Review passed.\"}",
        );

        assert_eq!(parsed.response, "Review passed.");
        assert_eq!(parsed.session_id.as_deref(), Some("abc"));
    }

    #[test]
    fn review_workpad_includes_required_operator_action_for_backend_startup_failure() {
        let job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "review backend failed: review backend startup failed: configured command: `gemini`; resolved executable: not found in worker PATH; suggested fix: configure `review_lane.gemini_command` with an absolute Gemini path; retry: rerun `review loop` after updating the workflow or environment.",
        );

        let workpad = render_review_workpad(&issue(), &job);

        assert!(workpad.contains("### Required Operator Action"));
        assert!(workpad.contains("Gemini backend health: `non_recovering_config`"));
        assert!(workpad.contains("Human intervention is required"));
        assert!(workpad.contains("Gemini Backend Health"));
        assert!(workpad.contains("must not"));
    }

    #[test]
    fn non_recovering_backend_startup_failure_needs_human_input() {
        let job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "review backend failed: review backend startup failed: configured command: `gemini`; resolved executable: not found in worker PATH",
        );

        let decision = review_gate_decision(&job);

        assert_eq!(decision.outcome, ReviewOutcome::NeedsHumanInput);
        assert_eq!(decision.target_state, Some("need_human_input"));
        assert!(decision.message.contains("non_recovering_config"));
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
        assert!(workpad.contains("<details>"));
        assert!(workpad.contains("<summary>Error: Gemini review command exited with status 1"));
        assert!(workpad.contains("```text\nGemini review command exited with status 1"));
        assert!(!workpad.contains("- Error: Gemini review command exited with status 1"));
        assert!(workpad.contains("Gemini Backend Health"));
        assert!(workpad.contains("non_recovering_config"));
        assert!(workpad.contains("Human intervention is required"));
    }

    #[test]
    fn classifies_quota_rate_limit_as_wait_and_retry() {
        let job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "HTTP 429 quota exceeded; retry-after: 2 minutes",
        );

        let diagnostic = gemini_review_health_diagnostic(&job).unwrap();
        let decision = review_gate_decision(&job);

        assert_eq!(
            diagnostic.category,
            GeminiReviewHealthCategory::QuotaRateLimit
        );
        assert_eq!(
            diagnostic.recovery_policy,
            GeminiReviewRecoveryPolicy::WaitAndRetry
        );
        assert_eq!(diagnostic.retry_after_ms, Some(120_000));
        assert_eq!(decision.outcome, ReviewOutcome::BackendUnavailable);
        assert_eq!(decision.target_state, Some("agent_review"));
    }

    #[test]
    fn classifies_transient_capacity_as_retry_with_backoff() {
        let job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "Gemini review command exited with status 1: HTTP 503 service unavailable, please retry later",
        );

        let diagnostic = gemini_review_health_diagnostic(&job).unwrap();
        let decision = review_gate_decision(&job);

        assert_eq!(
            diagnostic.category,
            GeminiReviewHealthCategory::TransientBackend
        );
        assert_eq!(
            diagnostic.recovery_policy,
            GeminiReviewRecoveryPolicy::RetryWithBackoff
        );
        assert_eq!(decision.outcome, ReviewOutcome::BackendUnavailable);
        assert_eq!(decision.target_state, Some("agent_review"));
    }

    #[test]
    fn classifies_policy_refusal_as_human_input() {
        let job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "Gemini review command exited with status 1: tool is not allowed by policy",
        );

        let diagnostic = gemini_review_health_diagnostic(&job).unwrap();
        let decision = review_gate_decision(&job);

        assert_eq!(
            diagnostic.category,
            GeminiReviewHealthCategory::NonRecoveringPolicy
        );
        assert_eq!(
            diagnostic.recovery_policy,
            GeminiReviewRecoveryPolicy::RequiresHumanInput
        );
        assert_eq!(decision.outcome, ReviewOutcome::NeedsHumanInput);
        assert_eq!(decision.target_state, Some("need_human_input"));
    }

    #[test]
    fn prelaunch_health_detects_missing_absolute_gemini_command() {
        let diagnostic = gemini_prelaunch_health_diagnostic(
            "/definitely/missing/jade-symphony-gemini",
            Some("gemini-3.1-pro-preview"),
            &["run_shell_command".into()],
        )
        .unwrap();

        assert_eq!(
            diagnostic.category,
            GeminiReviewHealthCategory::NonRecoveringConfig
        );
        assert_eq!(diagnostic.reason_code, "command_not_found");
    }

    #[test]
    fn repeated_failure_workpad_is_compact() {
        let mut job = ReviewJob::failed_unavailable(
            "#1",
            "gemini-cli",
            "HTTP 429 quota exceeded; retry-after: 60 seconds",
        );
        job.id = "gemini-repeat-2".into();
        let repeat = ReviewRepeatedFailureEvidence {
            repeat_count: 2,
            first_job_id: "gemini-repeat-1".into(),
            previous_job_id: "gemini-repeat-1".into(),
            signature: "quota_rate_limit:http_429".into(),
        };

        let workpad = render_repeated_review_failure_workpad(&issue(), &job, &repeat);

        assert!(workpad.contains("### Repeated Backend Failure"));
        assert!(workpad.contains("compact repeat line only"));
        assert!(workpad.contains("Same-cause repeat count: `2`"));
        assert!(!workpad.contains("```text"));
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
    fn finding_parser_ignores_positive_evidence_without_title_body_separator() {
        let findings = classify_findings(
            "Review Result: PASS\n\n[Confirmed] The PR diff shows the requested test isolation was implemented.\n[Confirmed] Bug: actual blocker",
        );

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, ReviewFindingClass::Confirmed);
        assert_eq!(findings[0].title, "Bug");
        assert_eq!(findings[0].body, "actual blocker");
    }

    #[test]
    fn rework_result_classifies_loose_confirmed_findings() {
        let findings = classify_findings(
            "Review Result: REWORK\n\n[Confirmed] The PR does not implement the parent execution gates for `Todo` issues.\n[Confirmed] Documentation changes fail to state the parent gate.",
        );

        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].class, ReviewFindingClass::Confirmed);
        assert_eq!(
            findings[0].title,
            "The PR does not implement the parent execution gates for `Todo` issues"
        );
        assert_eq!(findings[1].class, ReviewFindingClass::Confirmed);
    }

    #[test]
    fn rework_result_classifies_bulleted_loose_confirmed_findings() {
        let findings = classify_findings(
            "Review Result: REWORK\n\n### Findings\n- [Confirmed] The issue requires a controlled Merging rehearsal path.\n- [Confirmed] Safe merge-lane conflict repair was deferred.",
        );

        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].class, ReviewFindingClass::Confirmed);
        assert_eq!(
            findings[0].title,
            "The issue requires a controlled Merging rehearsal path"
        );
        assert_eq!(findings[1].class, ReviewFindingClass::Confirmed);
    }

    #[test]
    fn rework_result_without_parseable_findings_still_blocks_progress() {
        let findings = classify_findings(
            "Review Result: REWORK\n\nThe implementation is missing the required claim gate.",
        );

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, ReviewFindingClass::Confirmed);
        assert_eq!(findings[0].title, "Review result requires rework");
    }

    #[test]
    fn completed_rework_result_routes_to_rework() {
        let job = completed_gemini_review(
            "Review Result: REWORK\n\n[Confirmed] The PR does not implement the parent execution gates for `Todo` issues.",
        );

        let decision = review_gate_decision(&job);

        assert_eq!(decision.outcome, ReviewOutcome::NeedsRework);
        assert_eq!(decision.target_state, Some("rework"));
    }

    #[test]
    fn human_owned_uat_finding_does_not_block_agent_review_pass() {
        let job = completed_gemini_review(
            "Review Result: REWORK\n\n[Confirmed] Missing UAT: Live UAT with `main loop --write` was not run.",
        );

        let decision = review_gate_decision(&job);

        assert_eq!(decision.outcome, ReviewOutcome::PassedToHumanReview);
        assert_eq!(decision.target_state, Some("human_review"));
    }

    #[test]
    fn uat_implementation_deliverable_still_blocks_agent_review() {
        let job = completed_gemini_review(
            "Review Result: REWORK\n\n[Confirmed] Missing UAT fixture: The issue required implementing a controlled rehearsal path and UAT fixture, but the PR does not add it.",
        );

        let decision = review_gate_decision(&job);

        assert_eq!(decision.outcome, ReviewOutcome::NeedsRework);
        assert_eq!(decision.target_state, Some("rework"));
    }

    #[test]
    fn needs_context_result_without_parseable_findings_is_inconclusive() {
        let report = AgentReviewReport {
            reviewer_backend: "gemini-cli".into(),
            findings: classify_findings(
                "Review Result: NEEDS_CONTEXT\n\nThe linked PR could not be inspected.",
            ),
            summary: None,
            stdout: None,
            stderr: None,
            exit_status: Some("0".into()),
            session_id: None,
        };

        assert!(report.is_inconclusive());
        assert!(!report.blocks_progress());
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
    fn review_agent_passed_native_subissue_routes_to_merging() {
        let backend = FakeReviewBackend::new(FakeReviewOutcome::Pass);
        let temp = tempfile::tempdir().unwrap();
        let request = review_request_with_temp_roots(&temp);
        let job = backend.poll(backend.start(request).unwrap()).unwrap();
        let issue = native_subissue(issue());
        let decision = review_gate_decision_for_issue(&job, &issue);

        assert_eq!(decision.outcome, ReviewOutcome::PassedToMerging);
        assert_eq!(decision.target_state, Some("merging"));
        assert!(transition_allowed_for_review_agent("merging", &decision));
        assert!(!transition_allowed_for_review_agent(
            "human_review",
            &decision
        ));
    }

    #[test]
    fn review_agent_passed_native_subissue_exception_routes_to_human_review() {
        let backend = FakeReviewBackend::new(FakeReviewOutcome::Pass);
        let temp = tempfile::tempdir().unwrap();
        let request = review_request_with_temp_roots(&temp);
        let job = backend.poll(backend.start(request).unwrap()).unwrap();
        let mut issue = native_subissue(issue());
        issue.description =
            Some("Subissue Human Review Exception: operator-owned release risk.".into());
        let decision = review_gate_decision_for_issue(&job, &issue);

        assert_eq!(decision.outcome, ReviewOutcome::PassedToHumanReview);
        assert_eq!(decision.target_state, Some("human_review"));
    }

    #[test]
    fn review_agent_passed_parent_final_issue_routes_to_human_review() {
        let backend = FakeReviewBackend::new(FakeReviewOutcome::Pass);
        let temp = tempfile::tempdir().unwrap();
        let request = review_request_with_temp_roots(&temp);
        let job = backend.poll(backend.start(request).unwrap()).unwrap();
        let issue = parent_issue(issue());
        let decision = review_gate_decision_for_issue(&job, &issue);

        assert_eq!(decision.outcome, ReviewOutcome::PassedToHumanReview);
        assert_eq!(decision.target_state, Some("human_review"));
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
                    exit_status: None,
                    session_id: None,
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
                exit_status: None,
                session_id: None,
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
    fn review_workpad_uses_agent_review_template_with_note_and_target_state() {
        let job = ReviewJob {
            id: "gemini-1".into(),
            issue_ref: "#1".into(),
            backend: "gemini-cli".into(),
            state: ReviewJobState::Completed,
            artifact_path: Some("/tmp/review-output.json".into()),
            ledger_path: Some("/tmp/reviews/jobs/1-gemini.json".into()),
            report: Some(AgentReviewReport {
                reviewer_backend: "gemini-cli".into(),
                findings: Vec::new(),
                summary: Some("Review Result: PASS".into()),
                stdout: Some(
                    "{\"session_id\":\"gemini-session\",\"response\":\"Review Result: PASS\\n\\nAgent note body.\"}"
                        .into(),
                ),
                stderr: Some("warning only".into()),
                exit_status: Some("0".into()),
                session_id: Some("gemini-session".into()),
            }),
            error: None,
        };

        let body = render_review_workpad(&issue(), &job);

        assert!(body.contains("Generated at: `"));
        assert!(body.contains("GMT`"));
        assert!(body.contains("## Jade Symphony Agent Review Run"));
        assert!(body.contains("- Lane: `review`"));
        assert!(body.contains("- Actor role: `review_agent`"));
        assert!(body.contains("- Run ID: `gemini-1`"));
        assert!(body.contains("- PR: `https://github.com/Alive24/jade-symphony/pull/1`"));
        assert!(body.contains("Target state after review routing: `human_review`"));
        assert!(body.contains("- Result: `PassedToHumanReview`"));
        assert!(body.contains(
            "- Evidence summary: 0 parsed finding(s); artifact recorded; ledger recorded."
        ));
        assert!(body.contains("### Review Response"));
        assert!(body.contains("Agent note body."));
        assert!(body.contains("### Stdout"));
        assert!(body.contains("<details>"));
        assert!(body.contains("<summary>Stdout</summary>"));
        assert!(body.contains("### Stderr"));
        assert!(body.contains("<summary>Stderr</summary>"));
        assert!(body.contains("warning only"));
        assert!(body.contains("Review job ledger: `/tmp/reviews/jobs/1-gemini.json`"));
    }

    #[test]
    fn review_workpad_does_not_duplicate_parsed_findings_when_note_exists() {
        let response = "Review Result: REWORK\n\n### Findings\n- [Confirmed] Missing gate: The PR does not enforce the gate.\n\n### Evidence\n- `src/main.rs` was inspected.";
        let mut job = completed_gemini_review(response);
        job.report.as_mut().unwrap().stdout = Some(format!(
            "{{\"response\":{}}}",
            serde_json::to_string(response).unwrap()
        ));

        let body = render_review_workpad(&issue(), &job);

        assert!(body.contains("### Review Response"));
        assert!(body.contains("#### Findings"));
        assert!(body.contains("[Confirmed] Missing gate"));
        assert!(!body.contains("- Confirmed: Missing gate -"));
    }

    #[test]
    fn review_workpad_demotes_headings_inside_agent_review_note() {
        let report = AgentReviewReport {
            stdout: Some(
                "{\"response\":\"Review Result: PASS\\n\\nEvidence:\\n\\n## Details\\n\\n##### Deep Detail\"}".into(),
            ),
            ..Default::default()
        };

        let note = agent_review_note(&report).unwrap();

        assert!(note.contains("#### Review Result: PASS"));
        assert!(note.contains("#### Evidence"));
        assert!(note.contains("#### Details"));
        assert!(note.contains("##### Deep Detail"));
        assert!(!note.contains("\n## Details"));
    }

    #[test]
    fn review_workpad_formats_human_readable_gmt_timestamp() {
        assert_eq!(format_gmt_timestamp(0), "1970-01-01 00:00:00 GMT");
        assert_eq!(
            format_gmt_timestamp(1_779_085_565),
            "2026-05-18 06:26:05 GMT"
        );
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
                exit_status: None,
                session_id: None,
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
                exit_status: None,
                session_id: None,
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
                exit_status: None,
                session_id: None,
            }),
            error: None,
        };

        let body = render_review_workpad(&issue(), &job);

        assert!(body.contains("Review job ledger: `/tmp/reviews/jobs/1-job.json`"));
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
                exit_status: None,
                session_id: None,
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
