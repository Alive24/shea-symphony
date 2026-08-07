use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;
use crate::lane_claim::LaneClaim;
use crate::model::{normalize_state, TrackerIssue};
use crate::review::{ReviewJobLedgerRecord, ReviewJobState, ReviewOutcome};
use crate::runtime_state::{load_runtime_state, RuntimeState};
use crate::session_registry::{
    classify_session_record, load_session_registry, read_log_tail, session_registry_path,
    AgentSessionRecord, SessionStatus,
};

pub const DEFAULT_RECENT_REVIEW_JOBS: usize = 5;
const STDERR_LINE_LIMIT: usize = 5;
const STDERR_LINE_CHARS: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewStatusOptions {
    pub issue_filter: Option<String>,
    pub recent_limit: usize,
    pub verbose: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewStatusPayload {
    pub generated_at_ms: u64,
    pub issue_filter: Option<String>,
    pub recent_limit: usize,
    pub running_slots: Vec<ReviewStatusEntry>,
    pub recent_jobs: Vec<ReviewStatusEntry>,
    pub anomalies: Vec<ReviewStatusAnomaly>,
    pub sources: ReviewStatusSources,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewStatusSources {
    pub review_jobs_dir: PathBuf,
    pub session_registry_path: PathBuf,
    pub runtime_state_path: PathBuf,
    pub project_issue_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewStatusEntry {
    pub slot: Option<String>,
    pub issue_identifier: String,
    pub issue_title: Option<String>,
    pub issue_state: Option<String>,
    pub job_id: Option<String>,
    pub backend: Option<String>,
    pub pid: Option<u32>,
    pub pid_alive: Option<bool>,
    pub elapsed_ms: Option<u64>,
    pub artifact_path: Option<PathBuf>,
    pub artifact_exists: Option<bool>,
    pub ledger_path: Option<PathBuf>,
    pub claim_summary: Option<String>,
    pub last_event: Option<String>,
    pub review_outcome: Option<ReviewOutcome>,
    pub job_state: Option<ReviewJobState>,
    pub stderr_summary: Vec<String>,
    pub session_name: Option<String>,
    pub session_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewStatusAnomaly {
    pub code: String,
    pub severity: String,
    pub issue_identifier: Option<String>,
    pub job_id: Option<String>,
    pub message: String,
    pub detail: Option<String>,
}

#[derive(Debug, Error)]
pub enum ReviewStatusError {
    #[error("review status io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("review status json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
struct LedgerStatusRecord {
    record: ReviewJobLedgerRecord,
    recorded_at_ms: Option<u64>,
}

pub fn load_review_status(
    config: &RuntimeConfig,
    project_issues: &[TrackerIssue],
    options: &ReviewStatusOptions,
    now_ms: u64,
) -> Result<ReviewStatusPayload, ReviewStatusError> {
    let ledgers = load_review_job_ledgers(&review_jobs_dir(config))?;
    let session_registry = match load_session_registry(&session_registry_path(config)) {
        Ok(registry) => registry.sessions,
        Err(crate::session_registry::SessionRegistryError::Io(error))
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            Vec::new()
        }
        Err(error) => {
            return Err(ReviewStatusError::Io(std::io::Error::other(
                error.to_string(),
            )));
        }
    };
    let runtime_state = load_runtime_state(config)
        .map_err(|error| ReviewStatusError::Io(std::io::Error::other(error.to_string())))?;
    Ok(compose_review_status(
        config,
        project_issues,
        ledgers,
        &session_registry,
        runtime_state.as_ref(),
        options,
        now_ms,
    ))
}

fn compose_review_status(
    config: &RuntimeConfig,
    project_issues: &[TrackerIssue],
    mut ledgers: Vec<LedgerStatusRecord>,
    sessions: &[AgentSessionRecord],
    runtime_state: Option<&RuntimeState>,
    options: &ReviewStatusOptions,
    now_ms: u64,
) -> ReviewStatusPayload {
    ledgers.sort_by(|left, right| {
        right
            .recorded_at_ms
            .cmp(&left.recorded_at_ms)
            .then_with(|| right.record.job_id.cmp(&left.record.job_id))
    });
    let project_by_issue = project_issues
        .iter()
        .map(|issue| (normalize_issue_ref(&issue.identifier), issue))
        .collect::<BTreeMap<_, _>>();
    let issue_filter = options.issue_filter.as_deref().map(normalize_issue_ref);
    let mut anomalies = Vec::new();

    let matching_ledgers = ledgers
        .iter()
        .filter(|ledger| issue_matches_filter(&ledger.record.issue_ref, issue_filter.as_deref()))
        .collect::<Vec<_>>();

    let mut active_issue_keys = BTreeSet::new();
    let mut running_slots = Vec::new();
    for ledger in matching_ledgers
        .iter()
        .filter(|ledger| is_running_job_state(&ledger.record.state))
    {
        let entry = entry_from_ledger(
            ledger,
            project_by_issue
                .get(&normalize_issue_ref(&ledger.record.issue_ref))
                .copied(),
            now_ms,
        );
        active_issue_keys.insert(normalize_issue_ref(&entry.issue_identifier));
        append_entry_anomalies(&entry, config, &mut anomalies);
        running_slots.push(entry);
    }

    let review_sessions = sessions
        .iter()
        .filter(|session| session.lane.eq_ignore_ascii_case("review"))
        .filter(|session| {
            issue_matches_filter(
                session.issue_identifier.as_deref().unwrap_or_default(),
                issue_filter.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    for (index, session) in review_sessions.iter().enumerate() {
        if is_terminal_session_status(&session.status) {
            continue;
        }
        let entry = entry_from_session(
            config,
            session,
            index + 1,
            project_by_issue
                .get(&normalize_issue_ref(
                    session.issue_identifier.as_deref().unwrap_or_default(),
                ))
                .copied(),
            now_ms,
        );
        active_issue_keys.insert(normalize_issue_ref(&entry.issue_identifier));
        append_entry_anomalies(&entry, config, &mut anomalies);
        running_slots.push(entry);
    }

    if let Some(runtime) = runtime_state.filter(|runtime| {
        runtime
            .lane
            .as_deref()
            .is_some_and(|lane| lane.eq_ignore_ascii_case("review"))
    }) {
        if let Some(active) = runtime.active_issue.as_ref() {
            if issue_matches_filter(&active.identifier, issue_filter.as_deref()) {
                let key = normalize_issue_ref(&active.identifier);
                if !active_issue_keys.contains(&key) {
                    let entry =
                        entry_from_runtime(runtime, project_by_issue.get(&key).copied(), now_ms);
                    append_entry_anomalies(&entry, config, &mut anomalies);
                    active_issue_keys.insert(key);
                    running_slots.push(entry);
                }
            }
        }
    }

    for issue in project_issues.iter().filter(|issue| {
        issue_matches_filter(&issue.identifier, issue_filter.as_deref())
            && review_claim_value(issue).is_some()
    }) {
        let issue_key = normalize_issue_ref(&issue.identifier);
        let active_ledger = matching_ledgers.iter().any(|ledger| {
            normalize_issue_ref(&ledger.record.issue_ref) == issue_key
                && is_running_job_state(&ledger.record.state)
        });
        if !active_ledger && !active_issue_keys.contains(&issue_key) {
            anomalies.push(ReviewStatusAnomaly {
                code: "project_claim_without_active_job".into(),
                severity: "warning".into(),
                issue_identifier: Some(issue.identifier.clone()),
                job_id: None,
                message: "Project Review Agent claim exists but no active local review ledger or session was found.".into(),
                detail: review_claim_value(issue),
            });
            running_slots.push(entry_from_claim(issue));
        }
    }

    let mut recent_jobs = Vec::new();
    for ledger in matching_ledgers
        .iter()
        .filter(|ledger| !is_running_job_state(&ledger.record.state))
        .take(options.recent_limit)
    {
        let entry = entry_from_ledger(
            ledger,
            project_by_issue
                .get(&normalize_issue_ref(&ledger.record.issue_ref))
                .copied(),
            now_ms,
        );
        append_entry_anomalies(&entry, config, &mut anomalies);
        recent_jobs.push(entry);
    }

    ReviewStatusPayload {
        generated_at_ms: now_ms,
        issue_filter: options.issue_filter.clone(),
        recent_limit: options.recent_limit,
        running_slots,
        recent_jobs,
        anomalies,
        sources: ReviewStatusSources {
            review_jobs_dir: review_jobs_dir(config),
            session_registry_path: session_registry_path(config),
            runtime_state_path: crate::runtime_state::runtime_state_path(config),
            project_issue_count: project_issues.len(),
        },
    }
}

pub fn render_review_status_human(payload: &ReviewStatusPayload, verbose: bool) -> String {
    let mut lines = Vec::new();
    lines.push("Review Status".to_string());
    if let Some(filter) = &payload.issue_filter {
        lines.push(format!("Filter: issue={filter}"));
    }
    lines.push(String::new());
    lines.push("Running review slots".into());
    if payload.running_slots.is_empty() {
        lines.push("  none".into());
    } else {
        lines.push(format!(
            "{:<6} {:<7} {:<24} {:<18} {:<12} {:<8} {:<10} {:<22} {}",
            "SLOT", "ISSUE", "TITLE", "JOB", "BACKEND", "PID", "ELAPSED", "OUTCOME", "EVENT"
        ));
        for entry in &payload.running_slots {
            lines.push(render_entry_row(entry));
        }
    }
    lines.push(String::new());
    lines.push(format!(
        "Recent completed/failed jobs (last {})",
        payload.recent_limit
    ));
    if payload.recent_jobs.is_empty() {
        lines.push("  none".into());
    } else {
        lines.push(format!(
            "{:<6} {:<7} {:<24} {:<18} {:<12} {:<8} {:<10} {:<22} {}",
            "SLOT", "ISSUE", "TITLE", "JOB", "BACKEND", "PID", "ELAPSED", "OUTCOME", "EVENT"
        ));
        for entry in &payload.recent_jobs {
            lines.push(render_entry_row(entry));
        }
    }

    if !payload.anomalies.is_empty() {
        lines.push(String::new());
        lines.push("Anomalies".into());
        for anomaly in &payload.anomalies {
            lines.push(format!(
                "- [{}] {} issue={} job={} {}",
                anomaly.severity,
                anomaly.code,
                anomaly.issue_identifier.as_deref().unwrap_or("-"),
                anomaly.job_id.as_deref().unwrap_or("-"),
                anomaly.message
            ));
            if verbose {
                if let Some(detail) = &anomaly.detail {
                    lines.push(format!("  detail: {detail}"));
                }
            }
        }
    }

    lines.push(String::new());
    lines.push("Details".into());
    for entry in payload
        .running_slots
        .iter()
        .chain(payload.recent_jobs.iter())
        .filter(|entry| verbose || !entry.stderr_summary.is_empty())
    {
        lines.push(format!(
            "- {} {} artifact={} ledger={} claim={}",
            entry.issue_identifier,
            entry.job_id.as_deref().unwrap_or("-"),
            display_path(entry.artifact_path.as_ref()),
            display_path(entry.ledger_path.as_ref()),
            entry.claim_summary.as_deref().unwrap_or("-")
        ));
        if !entry.stderr_summary.is_empty() {
            lines.push("  stderr:".into());
            for line in &entry.stderr_summary {
                lines.push(format!("    {line}"));
            }
        }
    }
    if verbose {
        lines.push(String::new());
        lines.push(format!(
            "Sources: ledgers={} sessions={} runtime={}",
            payload.sources.review_jobs_dir.display(),
            payload.sources.session_registry_path.display(),
            payload.sources.runtime_state_path.display()
        ));
    }
    lines.join("\n")
}

pub fn render_project_inspect_review_summary(payload: &ReviewStatusPayload) -> Option<String> {
    let mut parts = Vec::new();
    if !payload.running_slots.is_empty() {
        parts.push(format!("running={}", payload.running_slots.len()));
    }
    if let Some(job) = payload.recent_jobs.first() {
        parts.push(format!(
            "latest_job={} outcome={}",
            job.job_id.as_deref().unwrap_or("-"),
            job.review_outcome
                .as_ref()
                .map(review_outcome_label)
                .unwrap_or("-")
        ));
    }
    if !payload.anomalies.is_empty() {
        parts.push(format!("anomalies={}", payload.anomalies.len()));
    }
    (!parts.is_empty()).then(|| {
        format!(
            "{}; use `review status <workflow> --issue {} --verbose` for lane runtime details",
            parts.join(" "),
            payload.issue_filter.as_deref().unwrap_or("#<issue>")
        )
    })
}

fn load_review_job_ledgers(path: &Path) -> Result<Vec<LedgerStatusRecord>, ReviewStatusError> {
    let mut records = Vec::new();
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(records),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let body = fs::read_to_string(&path)?;
        let mut record: ReviewJobLedgerRecord = serde_json::from_str(&body)?;
        record.ledger_path = path.clone();
        let recorded_at_ms = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(system_time_ms);
        records.push(LedgerStatusRecord {
            record,
            recorded_at_ms,
        });
    }
    Ok(records)
}

fn entry_from_ledger(
    ledger: &LedgerStatusRecord,
    issue: Option<&TrackerIssue>,
    now_ms: u64,
) -> ReviewStatusEntry {
    let record = &ledger.record;
    let elapsed_ms = record.started_at_ms.map(|started| {
        record
            .updated_at_ms
            .unwrap_or(now_ms)
            .saturating_sub(started)
    });
    let artifact_exists = record.artifact_path.as_ref().map(|path| path.exists());
    let stderr_summary = record
        .artifact_path
        .as_ref()
        .and_then(|path| stderr_summary_from_artifact(path).ok())
        .unwrap_or_default();
    ReviewStatusEntry {
        slot: None,
        issue_identifier: record.issue_ref.clone(),
        issue_title: Some(
            issue
                .map(|issue| issue.title.clone())
                .unwrap_or_else(|| record.issue_title.clone()),
        ),
        issue_state: issue.map(|issue| issue.state.clone()),
        job_id: Some(record.job_id.clone()),
        backend: Some(record.backend.clone()),
        pid: record.pid,
        pid_alive: record.pid.map(process_alive),
        elapsed_ms,
        artifact_path: record.artifact_path.clone(),
        artifact_exists,
        ledger_path: Some(record.ledger_path.clone()),
        claim_summary: issue
            .and_then(review_claim_value)
            .map(compact_claim_summary),
        last_event: record
            .summary
            .clone()
            .or_else(|| record.error.clone())
            .or_else(|| Some(format!("{:?}", record.state))),
        review_outcome: Some(record.decision_outcome),
        job_state: Some(record.state.clone()),
        stderr_summary,
        session_name: None,
        session_status: None,
    }
}

fn entry_from_session(
    config: &RuntimeConfig,
    session: &AgentSessionRecord,
    index: usize,
    issue: Option<&TrackerIssue>,
    now_ms: u64,
) -> ReviewStatusEntry {
    let pane_tail = crate::session_registry::capture_tmux_pane_tail(
        &config.tmux.command,
        &session.pane_target,
        40,
    )
    .ok();
    let log_tail = read_log_tail(&session.log_path, 40).ok().flatten();
    let probe = classify_session_record(
        session,
        pane_tail.as_deref(),
        log_tail.as_deref(),
        now_ms,
        15 * 60 * 1000,
    );
    let pid = tmux_pane_pid(&config.tmux.command, &session.pane_target);
    let issue_identifier = session
        .issue_identifier
        .clone()
        .or_else(|| issue.map(|issue| issue.identifier.clone()))
        .unwrap_or_else(|| "unknown".into());
    ReviewStatusEntry {
        slot: Some(format!("session-{index}")),
        issue_identifier,
        issue_title: session
            .issue_title
            .clone()
            .or_else(|| issue.map(|issue| issue.title.clone())),
        issue_state: issue.map(|issue| issue.state.clone()),
        job_id: session.run_id.clone(),
        backend: Some(session.backend.clone()),
        pid,
        pid_alive: pid.map(process_alive),
        elapsed_ms: Some(now_ms.saturating_sub(session.started_at_ms)),
        artifact_path: Some(session.prompt_artifact_path.clone()),
        artifact_exists: Some(session.prompt_artifact_path.exists()),
        ledger_path: None,
        claim_summary: session
            .claim_value
            .clone()
            .or_else(|| issue.and_then(review_claim_value))
            .map(compact_claim_summary),
        last_event: Some(probe.evidence),
        review_outcome: Some(ReviewOutcome::StillRunning),
        job_state: Some(ReviewJobState::Running),
        stderr_summary: sanitize_stderr_lines(log_tail.as_deref().unwrap_or_default()),
        session_name: Some(session.session_name.clone()),
        session_status: Some(probe.status.as_str().into()),
    }
}

fn entry_from_runtime(
    runtime: &RuntimeState,
    issue: Option<&TrackerIssue>,
    now_ms: u64,
) -> ReviewStatusEntry {
    let active = runtime.active_issue.as_ref();
    ReviewStatusEntry {
        slot: Some("runtime".into()),
        issue_identifier: active
            .map(|active| active.identifier.clone())
            .or_else(|| issue.map(|issue| issue.identifier.clone()))
            .unwrap_or_else(|| "unknown".into()),
        issue_title: issue.map(|issue| issue.title.clone()),
        issue_state: issue.map(|issue| issue.state.clone()),
        job_id: runtime.run_id.clone(),
        backend: Some(runtime.backend.clone()),
        pid: None,
        pid_alive: None,
        elapsed_ms: runtime
            .updated_at_ms
            .map(|updated_at| now_ms.saturating_sub(updated_at)),
        artifact_path: None,
        artifact_exists: None,
        ledger_path: None,
        claim_summary: issue
            .and_then(review_claim_value)
            .map(compact_claim_summary),
        last_event: runtime.last_event.clone(),
        review_outcome: Some(ReviewOutcome::StillRunning),
        job_state: Some(ReviewJobState::Running),
        stderr_summary: Vec::new(),
        session_name: runtime.backend_session_id.clone(),
        session_status: Some("runtime_active".into()),
    }
}

fn entry_from_claim(issue: &TrackerIssue) -> ReviewStatusEntry {
    let claim = review_claim_value(issue).unwrap_or_default();
    ReviewStatusEntry {
        slot: Some("claim".into()),
        issue_identifier: issue.identifier.clone(),
        issue_title: Some(issue.title.clone()),
        issue_state: Some(issue.state.clone()),
        job_id: LaneClaim::parse(&claim).ok().map(|claim| claim.run),
        backend: Some("project-claim".into()),
        pid: None,
        pid_alive: None,
        elapsed_ms: None,
        artifact_path: None,
        artifact_exists: None,
        ledger_path: None,
        claim_summary: Some(compact_claim_summary(claim)),
        last_event: Some("Project Review Agent claim without matching local active job".into()),
        review_outcome: Some(ReviewOutcome::StillRunning),
        job_state: Some(ReviewJobState::Running),
        stderr_summary: Vec::new(),
        session_name: None,
        session_status: Some("claim_only".into()),
    }
}

fn append_entry_anomalies(
    entry: &ReviewStatusEntry,
    config: &RuntimeConfig,
    anomalies: &mut Vec<ReviewStatusAnomaly>,
) {
    let issue = Some(entry.issue_identifier.clone());
    let job = entry.job_id.clone();
    if entry.job_state.as_ref().is_some_and(is_running_job_state) {
        if entry.pid.is_none() {
            anomalies.push(anomaly(
                "running_job_missing_pid",
                "warning",
                issue.clone(),
                job.clone(),
                "Review job appears to be running but no pid was recorded.",
                None,
            ));
        } else if entry.pid_alive == Some(false) {
            anomalies.push(anomaly(
                "running_job_dead_pid",
                "warning",
                issue.clone(),
                job.clone(),
                "Review job appears to be running but the recorded pid is not alive.",
                None,
            ));
        }
        if entry
            .elapsed_ms
            .is_some_and(|elapsed| elapsed > config.review.timeout_ms)
        {
            anomalies.push(anomaly(
                "running_job_exceeds_threshold",
                "warning",
                issue.clone(),
                job.clone(),
                "Review job elapsed time exceeds the configured review timeout.",
                entry.elapsed_ms.map(|elapsed| {
                    format!(
                        "elapsed_ms={elapsed} timeout_ms={}",
                        config.review.timeout_ms
                    )
                }),
            ));
        }
        if entry.issue_state.as_ref().is_some_and(|state| {
            normalize_state(state) != normalize_state(&config.tracker.state_map.agent_review)
        }) {
            anomalies.push(anomaly(
                "running_job_issue_state_mismatch",
                "warning",
                issue.clone(),
                job.clone(),
                "Review job appears active but issue is no longer in Agent Review.",
                entry.issue_state.clone(),
            ));
        }
    }
    if entry.artifact_path.is_none() || entry.artifact_exists == Some(false) {
        anomalies.push(anomaly(
            "review_artifact_missing",
            "warning",
            issue.clone(),
            job.clone(),
            "Review artifact path is missing or does not exist.",
            entry
                .artifact_path
                .as_ref()
                .map(|path| path.display().to_string()),
        ));
    }
    if entry.review_outcome.as_ref().is_some_and(|outcome| {
        matches!(
            outcome,
            ReviewOutcome::BackendUnavailable
                | ReviewOutcome::NeedsHumanInput
                | ReviewOutcome::InconclusiveNeedsRework
                | ReviewOutcome::NeedsRework
        )
    }) {
        anomalies.push(anomaly(
            "review_outcome_attention",
            "warning",
            issue.clone(),
            job.clone(),
            "Last review outcome needs operator or Main-lane attention.",
            entry
                .review_outcome
                .as_ref()
                .map(review_outcome_label)
                .map(str::to_string),
        ));
    }
    let error_text = [
        entry.last_event.as_deref().unwrap_or_default(),
        &entry.stderr_summary.join("\n"),
    ]
    .join("\n")
    .to_ascii_lowercase();
    if contains_backend_attention(&error_text) {
        anomalies.push(anomaly(
            "review_backend_attention",
            "warning",
            issue,
            job,
            "Review backend appears unavailable or blocked by binary/auth/configuration.",
            entry.last_event.clone(),
        ));
    }
}

fn anomaly(
    code: &str,
    severity: &str,
    issue_identifier: Option<String>,
    job_id: Option<String>,
    message: &str,
    detail: Option<String>,
) -> ReviewStatusAnomaly {
    ReviewStatusAnomaly {
        code: code.into(),
        severity: severity.into(),
        issue_identifier,
        job_id,
        message: message.into(),
        detail,
    }
}

fn render_entry_row(entry: &ReviewStatusEntry) -> String {
    format!(
        "{:<6} {:<7} {:<24} {:<18} {:<12} {:<8} {:<10} {:<22} {}",
        truncate(entry.slot.as_deref().unwrap_or("-"), 6),
        truncate(&entry.issue_identifier, 7),
        truncate(entry.issue_title.as_deref().unwrap_or("-"), 24),
        truncate(entry.job_id.as_deref().unwrap_or("-"), 18),
        truncate(entry.backend.as_deref().unwrap_or("-"), 12),
        entry
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "-".into()),
        entry
            .elapsed_ms
            .map(format_duration)
            .unwrap_or_else(|| "-".into()),
        truncate(
            entry
                .review_outcome
                .as_ref()
                .map(review_outcome_label)
                .unwrap_or("-"),
            22,
        ),
        truncate(entry.last_event.as_deref().unwrap_or("-"), 80)
    )
}

fn stderr_summary_from_artifact(path: &Path) -> Result<Vec<String>, ReviewStatusError> {
    let body = fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&body)?;
    let stderr = value
        .get("stderr")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    Ok(sanitize_stderr_lines(stderr))
}

fn sanitize_stderr_lines(stderr: &str) -> Vec<String> {
    let cleaned = strip_ansi_and_control(stderr);
    cleaned
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .rev()
        .take(STDERR_LINE_LIMIT)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| truncate(line, STDERR_LINE_CHARS))
        .collect()
}

fn strip_ansi_and_control(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        }
        out.push(ch);
    }
    out
}

fn review_claim_value(issue: &TrackerIssue) -> Option<String> {
    issue
        .project_fields
        .get("Review Agent")
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| Some(value.to_string()))
        })
        .filter(|value| !value.trim().is_empty() && value.trim() != "null")
}

fn compact_claim_summary(value: String) -> String {
    LaneClaim::parse(&value)
        .map(|claim| {
            format!(
                "{} run={} state={} worker={}",
                claim.lane.as_str(),
                claim.run,
                claim.state.as_str(),
                claim.worker.as_deref().unwrap_or("-")
            )
        })
        .unwrap_or_else(|_| truncate(&value, 120))
}

fn is_running_job_state(state: &ReviewJobState) -> bool {
    matches!(state, ReviewJobState::Queued | ReviewJobState::Running)
}

fn is_terminal_session_status(status: &SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Completed | SessionStatus::Failed | SessionStatus::Recorded
    )
}

fn issue_matches_filter(issue_ref: &str, filter: Option<&str>) -> bool {
    filter
        .map(|filter| normalize_issue_ref(issue_ref) == filter)
        .unwrap_or(true)
}

fn normalize_issue_ref(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let bare = trimmed.trim_start_matches('#');
    if bare.chars().all(|ch| ch.is_ascii_digit()) {
        format!("#{bare}")
    } else {
        trimmed.to_ascii_lowercase()
    }
}

fn review_jobs_dir(config: &RuntimeConfig) -> PathBuf {
    config.observability.logs_root.join("reviews").join("jobs")
}

fn system_time_ms(time: std::time::SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

fn tmux_pane_pid(tmux_command: &str, target: &str) -> Option<u32> {
    let output = Command::new(tmux_command)
        .args(["display-message", "-p", "-t", target, "#{pane_pid}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .ok()
}

fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn contains_backend_attention(value: &str) -> bool {
    [
        "not found",
        "missing binary",
        "permission denied",
        "auth",
        "authentication",
        "api key",
        "config",
        "backend unavailable",
        "startup failed",
        "spawn error",
        "usage limit",
        "quota",
        "rate limit",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

fn review_outcome_label(outcome: &ReviewOutcome) -> &'static str {
    match outcome {
        ReviewOutcome::PassedToHumanReview => "passed_to_human_review",
        ReviewOutcome::PassedToMerging => "passed_to_merging",
        ReviewOutcome::NeedsRework => "needs_rework",
        ReviewOutcome::InconclusiveNeedsRework => "inconclusive_needs_rework",
        ReviewOutcome::NeedsHumanInput => "needs_human_input",
        ReviewOutcome::BackendUnavailable => "backend_unavailable",
        ReviewOutcome::StillRunning => "still_running",
        ReviewOutcome::Cancelled => "cancelled",
    }
}

fn format_duration(ms: u64) -> String {
    let seconds = ms / 1_000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m{}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{}m", seconds / 3_600, (seconds % 3_600) / 60)
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let keep = max_chars - 3;
    format!("{}...", value.chars().take(keep).collect::<String>())
}

fn display_path(path: Option<&PathBuf>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "-".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuntimeConfig;
    use crate::model::TrackerIssue;

    fn config() -> RuntimeConfig {
        let workflow = crate::workflow::WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nreview_lane:\n  timeout_ms: 1000\nobservability:\n  logs_root: /tmp/shea-review-status-test/logs\ntmux:\n  command: /bin/false\n---\nPrompt",
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn issue(identifier: &str, state: &str) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: identifier.into(),
            item_id: None,
            identifier: identifier.into(),
            title: format!("Issue {identifier}"),
            description: None,
            url: None,
            state: state.into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: None,
            linked_pull_requests: Vec::new(),
            blocked_by: Vec::new(),
            project_fields: BTreeMap::new(),
            created_at: None,
            updated_at: None,
        }
    }

    fn ledger(issue_ref: &str, job_id: &str, state: ReviewJobState) -> LedgerStatusRecord {
        LedgerStatusRecord {
            recorded_at_ms: Some(2_000),
            record: ReviewJobLedgerRecord {
                issue_ref: issue_ref.into(),
                issue_title: format!("Issue {issue_ref}"),
                job_id: job_id.into(),
                worker_key: format!("review:{issue_ref}:gemini-cli"),
                backend: "gemini-cli".into(),
                state,
                pid: None,
                started_at_ms: Some(0),
                updated_at_ms: Some(2_000),
                artifact_path: Some(PathBuf::from("/tmp/missing-artifact.json")),
                ledger_path: PathBuf::from(format!("/tmp/{job_id}.json")),
                backend_session_id: None,
                decision_outcome: ReviewOutcome::StillRunning,
                decision_target_state: Some("agent_review".into()),
                summary: Some("Review is running".into()),
                error: None,
                finding_count: 0,
                gemini_health: None,
            },
        }
    }

    #[test]
    fn running_ledger_reports_pid_and_artifact_anomalies() {
        let config = config();
        let payload = compose_review_status(
            &config,
            &[issue("#313", "Agent Review")],
            vec![ledger("#313", "job-1", ReviewJobState::Running)],
            &[],
            None,
            &ReviewStatusOptions {
                issue_filter: Some("#313".into()),
                recent_limit: 5,
                verbose: false,
            },
            5_000,
        );

        assert_eq!(payload.running_slots.len(), 1);
        assert!(payload
            .anomalies
            .iter()
            .any(|anomaly| anomaly.code == "running_job_missing_pid"));
        assert!(payload
            .anomalies
            .iter()
            .any(|anomaly| anomaly.code == "review_artifact_missing"));
        assert!(payload
            .anomalies
            .iter()
            .any(|anomaly| anomaly.code == "running_job_exceeds_threshold"));
    }

    #[test]
    fn recent_limit_and_issue_filter_are_applied() {
        let config = config();
        let mut first = ledger("#1", "job-1", ReviewJobState::Completed);
        first.record.decision_outcome = ReviewOutcome::PassedToHumanReview;
        let mut second = ledger("#2", "job-2", ReviewJobState::Failed);
        second.record.decision_outcome = ReviewOutcome::NeedsHumanInput;

        let payload = compose_review_status(
            &config,
            &[issue("#1", "Human Review"), issue("#2", "Agent Review")],
            vec![first, second],
            &[],
            None,
            &ReviewStatusOptions {
                issue_filter: Some("2".into()),
                recent_limit: 1,
                verbose: false,
            },
            5_000,
        );

        assert!(payload.running_slots.is_empty());
        assert_eq!(payload.recent_jobs.len(), 1);
        assert_eq!(payload.recent_jobs[0].issue_identifier, "#2");
    }

    #[test]
    fn project_claim_without_local_job_is_highlighted() {
        let config = config();
        let mut issue = issue("#313", "Agent Review");
        issue.project_fields.insert(
            "Review Agent".into(),
            serde_json::Value::String(
                "v=1 lane=review actor=gemini worker=\"review:#313:gemini-cli\" source=loop issue=#313 run=review-run state=active thread=unknown registry=run/review-run".into(),
            ),
        );

        let payload = compose_review_status(
            &config,
            &[issue],
            Vec::new(),
            &[],
            None,
            &ReviewStatusOptions {
                issue_filter: None,
                recent_limit: 5,
                verbose: false,
            },
            5_000,
        );

        assert_eq!(payload.running_slots.len(), 1);
        assert!(payload
            .anomalies
            .iter()
            .any(|anomaly| anomaly.code == "project_claim_without_active_job"));
    }

    #[test]
    fn stderr_summary_sanitizes_ansi_control_and_tails_five_lines() {
        let stderr = "\u{1b}]0;title\u{7}\u{1b}[31mone\u{1b}[0m\ntwo\nthree\nfour\nfive\nsix\u{7}";
        let lines = sanitize_stderr_lines(stderr);

        assert_eq!(lines, vec!["two", "three", "four", "five", "six"]);
    }
}
