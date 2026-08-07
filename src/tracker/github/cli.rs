use std::cell::RefCell;
use std::fs;
use std::process::{Command, Output, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::config::RuntimeConfig;

use super::super::{
    classify_project_state_error, classify_project_state_failure_message, graphql_error_message,
    ProjectStateFailureKind, TrackerError,
};

pub(in crate::tracker) fn gh_available() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GithubCliApi {
    Graphql,
    RestJson,
}

#[derive(Clone, Default)]
struct GithubGraphqlRequestContext {
    action: String,
    cancellation: Option<Arc<AtomicBool>>,
}

thread_local! {
    static GITHUB_GRAPHQL_REQUEST_CONTEXT: RefCell<GithubGraphqlRequestContext> =
        RefCell::new(GithubGraphqlRequestContext::default());
}

struct GithubGraphqlRequestContextGuard(Option<GithubGraphqlRequestContext>);

impl Drop for GithubGraphqlRequestContextGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            GITHUB_GRAPHQL_REQUEST_CONTEXT.with(|context| {
                context.replace(previous);
            });
        }
    }
}

/// Runs a tracker operation with bounded, secret-free GitHub GraphQL action
/// evidence and optional cancellation for a shared rate-limit cooldown.
///
/// The context is thread-local while the cooldown is process-local, matching
/// one workflow/runtime profile per Shea process without creating durable or
/// cross-repository coordination.
pub fn with_github_graphql_request_context<T>(
    action: impl Into<String>,
    cancellation: Option<Arc<AtomicBool>>,
    operation: impl FnOnce() -> T,
) -> T {
    let previous = GITHUB_GRAPHQL_REQUEST_CONTEXT.with(|context| {
        context.replace(GithubGraphqlRequestContext {
            action: action.into(),
            cancellation,
        })
    });
    let _guard = GithubGraphqlRequestContextGuard(Some(previous));
    operation()
}

fn github_graphql_request_context() -> GithubGraphqlRequestContext {
    GITHUB_GRAPHQL_REQUEST_CONTEXT.with(|context| context.borrow().clone())
}

const GITHUB_GRAPHQL_COOLDOWN_FALLBACK_MS: u64 = 1_000;
const GITHUB_GRAPHQL_COOLDOWN_MAX_MS: u64 = 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubGraphqlBudgetEvidence {
    operation: String,
    action: String,
    cost: Option<i64>,
    remaining: Option<i64>,
    reset_at: Option<String>,
    reset_at_ms: Option<u64>,
}

impl GithubGraphqlBudgetEvidence {
    fn from_response(args: &[String], response: &serde_json::Value) -> Self {
        let reset_at = response
            .pointer("/data/rateLimit/resetAt")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        Self {
            operation: github_graphql_operation(args),
            action: github_graphql_action(),
            cost: response
                .pointer("/data/rateLimit/cost")
                .and_then(serde_json::Value::as_i64),
            remaining: response
                .pointer("/data/rateLimit/remaining")
                .and_then(serde_json::Value::as_i64),
            reset_at_ms: reset_at
                .as_deref()
                .and_then(parse_github_graphql_reset_at_ms),
            reset_at,
        }
    }

    fn without_response(args: &[String]) -> Self {
        Self {
            operation: github_graphql_operation(args),
            action: github_graphql_action(),
            cost: None,
            remaining: None,
            reset_at: None,
            reset_at_ms: None,
        }
    }

    fn compact(&self) -> String {
        format!(
            "operation={} action={} cost={} remaining={} reset_at={}",
            self.operation,
            self.action,
            self.cost
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".into()),
            self.remaining
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".into()),
            self.reset_at.as_deref().unwrap_or("unavailable")
        )
    }
}

#[derive(Debug, Default)]
struct GithubGraphqlCooldownState {
    until_ms: u64,
    reset_at: Option<String>,
    decision_id: u64,
    announced_decision_id: u64,
}

#[derive(Debug, Default)]
struct GithubGraphqlCooldown {
    state: Mutex<GithubGraphqlCooldownState>,
}

impl GithubGraphqlCooldown {
    fn schedule(&self, evidence: &GithubGraphqlBudgetEvidence, now_ms: u64) -> bool {
        let provider_deadline = evidence.reset_at_ms.filter(|deadline| *deadline > now_ms);
        let until_ms = provider_deadline
            .unwrap_or_else(|| now_ms.saturating_add(GITHUB_GRAPHQL_COOLDOWN_FALLBACK_MS))
            .min(now_ms.saturating_add(GITHUB_GRAPHQL_COOLDOWN_MAX_MS));
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.until_ms >= until_ms && state.until_ms > now_ms {
            return false;
        }
        state.until_ms = until_ms;
        state.reset_at = evidence.reset_at.clone();
        state.decision_id = state.decision_id.saturating_add(1);
        eprintln!(
            "github_graphql_cooldown scope=process decision=wait_until_reset decision_id={} delay_ms={} {}",
            state.decision_id,
            until_ms.saturating_sub(now_ms),
            evidence.compact()
        );
        true
    }

    fn wait(&self) -> Result<(), TrackerError> {
        loop {
            let now_ms = unix_time_ms();
            let (until_ms, decision_id, reset_at, announce) = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                if state.until_ms <= now_ms {
                    state.until_ms = 0;
                    state.reset_at = None;
                    return Ok(());
                }
                let announce = state.announced_decision_id != state.decision_id;
                if announce {
                    state.announced_decision_id = state.decision_id;
                }
                (
                    state.until_ms,
                    state.decision_id,
                    state.reset_at.clone(),
                    announce,
                )
            };
            if announce {
                eprintln!(
                    "github_graphql_cooldown scope=process status=waiting decision_id={} due_in_ms={} reset_at={}",
                    decision_id,
                    until_ms.saturating_sub(now_ms),
                    reset_at.as_deref().unwrap_or("unavailable")
                );
            }
            if github_graphql_request_context()
                .cancellation
                .is_some_and(|cancellation| cancellation.load(Ordering::SeqCst))
            {
                return Err(TrackerError::IntegrationUnavailable(
                    "GitHub GraphQL shared rate-limit cooldown cancelled".into(),
                ));
            }
            thread::sleep(Duration::from_millis(
                until_ms.saturating_sub(now_ms).min(250),
            ));
        }
    }
}

fn github_graphql_cooldown() -> &'static GithubGraphqlCooldown {
    static COOLDOWN: OnceLock<GithubGraphqlCooldown> = OnceLock::new();
    COOLDOWN.get_or_init(GithubGraphqlCooldown::default)
}

fn github_graphql_action() -> String {
    let action = github_graphql_request_context().action;
    if action.trim().is_empty() {
        "tracker.unspecified".into()
    } else {
        action
    }
}

fn github_graphql_operation(args: &[String]) -> String {
    let query = args
        .iter()
        .find_map(|arg| arg.strip_prefix("query="))
        .unwrap_or_default();
    let mut tokens = query.split_whitespace();
    while let Some(token) = tokens.next() {
        if matches!(token, "query" | "mutation") {
            return tokens
                .next()
                .unwrap_or("anonymous")
                .split(['(', '{'])
                .next()
                .unwrap_or("anonymous")
                .to_string();
        }
    }
    "anonymous".into()
}

fn parse_github_graphql_reset_at_ms(value: &str) -> Option<u64> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339).ok()?;
    u64::try_from(timestamp.unix_timestamp_nanos() / 1_000_000).ok()
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

impl GithubCliApi {
    fn operation_label(self) -> &'static str {
        match self {
            Self::Graphql => "GitHub GraphQL operation",
            Self::RestJson => "GitHub REST operation",
        }
    }

    fn invalid_json_label(self) -> &'static str {
        match self {
            Self::Graphql => "invalid GitHub GraphQL JSON",
            Self::RestJson => "invalid GitHub API JSON",
        }
    }

    fn validate_response(self, response: &serde_json::Value) -> Result<(), TrackerError> {
        if self == Self::Graphql {
            if let Some(message) = graphql_error_message(response) {
                return Err(TrackerError::IntegrationUnavailable(message));
            }
        }
        Ok(())
    }
}

pub(in crate::tracker) struct GithubCliAccess;

impl GithubCliAccess {
    const MAX_ATTEMPTS: usize = 2;
    const TIMEOUT: Duration = Duration::from_secs(10);

    fn run_json(api: GithubCliApi, args: Vec<String>) -> Result<serde_json::Value, TrackerError> {
        let mut last_error = None;

        for attempt in 1..=Self::MAX_ATTEMPTS {
            match Self::run_json_once(api, &args) {
                Ok(response) => return Ok(response),
                Err(error) if project_state_error_is_retryable(&error) => {
                    last_error = Some(error);
                    if attempt < Self::MAX_ATTEMPTS {
                        if !last_error.as_ref().is_some_and(|error| {
                            classify_project_state_error(error)
                                == ProjectStateFailureKind::RateLimit
                        }) {
                            thread::sleep(project_state_retry_delay(attempt));
                        }
                    } else {
                        break;
                    }
                }
                Err(error) => return Err(error),
            }
        }

        let error = last_error.unwrap_or_else(|| {
            TrackerError::IntegrationUnavailable(format!("{} failed", api.operation_label()))
        });
        let kind = classify_project_state_error(&error);
        Err(TrackerError::IntegrationUnavailable(format!(
            "{} failed after {} attempts kind={}: {error}",
            api.operation_label(),
            Self::MAX_ATTEMPTS,
            kind.as_str()
        )))
    }

    pub(in crate::tracker) fn run_status(
        args: Vec<String>,
        operation: &str,
    ) -> Result<(), TrackerError> {
        let output = run_gh_command(&args, operation)?;

        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let kind = classify_project_state_failure_message(&message);
            return Err(TrackerError::IntegrationUnavailable(format!(
                "{operation} failed kind={}: {message}",
                kind.as_str()
            )));
        }

        Ok(())
    }

    fn run_json_once(
        api: GithubCliApi,
        args: &[String],
    ) -> Result<serde_json::Value, TrackerError> {
        if api == GithubCliApi::Graphql {
            github_graphql_cooldown().wait()?;
        }
        let output = run_gh_command(args, api.operation_label())?;

        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let kind = classify_project_state_failure_message(&message);
            if api == GithubCliApi::Graphql && kind == ProjectStateFailureKind::RateLimit {
                let evidence = GithubGraphqlBudgetEvidence::without_response(args);
                github_graphql_cooldown().schedule(&evidence, unix_time_ms());
                return Err(TrackerError::IntegrationUnavailable(format!(
                    "{} failed kind={}: {}; github_graphql_budget {}",
                    api.operation_label(),
                    kind.as_str(),
                    message,
                    evidence.compact()
                )));
            }
            return Err(TrackerError::IntegrationUnavailable(format!(
                "{} failed kind={}: {message}",
                api.operation_label(),
                kind.as_str()
            )));
        }

        let response: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                TrackerError::Payload(format!("{}: {error}", api.invalid_json_label()))
            })?;
        if api == GithubCliApi::Graphql {
            let evidence = GithubGraphqlBudgetEvidence::from_response(args, &response);
            eprintln!("github_graphql_budget {}", evidence.compact());
            if evidence.remaining == Some(0) {
                github_graphql_cooldown().schedule(&evidence, unix_time_ms());
            }
            if let Err(error) = api.validate_response(&response) {
                if classify_project_state_error(&error) == ProjectStateFailureKind::RateLimit {
                    github_graphql_cooldown().schedule(&evidence, unix_time_ms());
                }
                return Err(TrackerError::IntegrationUnavailable(format!(
                    "{error}; github_graphql_budget {}",
                    evidence.compact()
                )));
            }
        } else {
            api.validate_response(&response)?;
        }
        Ok(response)
    }
}

fn run_gh_command(args: &[String], operation: &str) -> Result<Output, TrackerError> {
    run_command_with_timeout("gh", args, operation, GithubCliAccess::TIMEOUT)
}

pub(in crate::tracker) fn run_command_with_timeout(
    program: &str,
    args: &[String],
    operation: &str,
    timeout: Duration,
) -> Result<Output, TrackerError> {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base = format!("shea-symphony-command-{}-{suffix}", std::process::id());
    let stdout_path = std::env::temp_dir().join(format!("{base}.stdout"));
    let stderr_path = std::env::temp_dir().join(format!("{base}.stderr"));
    let stdout_file = fs::File::create(&stdout_path).map_err(|error| {
        TrackerError::IntegrationUnavailable(format!("{operation} stdout capture failed: {error}"))
    })?;
    let stderr_file = fs::File::create(&stderr_path).map_err(|error| {
        let _ = fs::remove_file(&stdout_path);
        TrackerError::IntegrationUnavailable(format!("{operation} stderr capture failed: {error}"))
    })?;

    let mut child = Command::new(program)
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|error| {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            TrackerError::IntegrationUnavailable(error.to_string())
        })?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = fs::read(&stdout_path).map_err(|error| {
                    TrackerError::IntegrationUnavailable(format!(
                        "{operation} stdout read failed: {error}"
                    ))
                })?;
                let stderr = fs::read(&stderr_path).map_err(|error| {
                    TrackerError::IntegrationUnavailable(format!(
                        "{operation} stderr read failed: {error}"
                    ))
                })?;
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Err(TrackerError::IntegrationUnavailable(format!(
                    "{operation} timed out after {}ms",
                    timeout.as_millis()
                )));
            }
            Ok(None) => {
                let elapsed = started.elapsed();
                if elapsed >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_file(&stdout_path);
                    let _ = fs::remove_file(&stderr_path);
                    return Err(TrackerError::IntegrationUnavailable(format!(
                        "{operation} timed out after {}ms",
                        timeout.as_millis()
                    )));
                }
                thread::sleep((timeout - elapsed).min(Duration::from_millis(100)));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Err(TrackerError::IntegrationUnavailable(format!(
                    "{operation} wait failed: {error}"
                )));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::tracker) enum GithubAuthMode {
    Fixture,
    EnvToken,
    GhCli,
    MissingGh,
    Unauthenticated { reason: Option<String> },
}

pub(in crate::tracker) fn github_auth_mode<F>(
    config: &RuntimeConfig,
    gh_installed: bool,
    gh_auth_check: F,
) -> GithubAuthMode
where
    F: FnOnce() -> Result<(), String>,
{
    if config.tracker.fixture_path.is_some() {
        return GithubAuthMode::Fixture;
    }

    if config.tracker.api_key.is_some() {
        return GithubAuthMode::EnvToken;
    }

    if !gh_installed {
        return GithubAuthMode::MissingGh;
    }

    match gh_auth_check() {
        Ok(()) => GithubAuthMode::GhCli,
        Err(error) => GithubAuthMode::Unauthenticated {
            reason: Some(error),
        },
    }
}

pub(in crate::tracker) fn github_graphql_auth_smoke() -> Result<(), String> {
    run_gh_graphql(vec![
        "api".into(),
        "graphql".into(),
        "-f".into(),
        "query=query { viewer { login } }".into(),
    ])
    .map(|_| ())
    .map_err(|error| error.to_string())
}

pub(in crate::tracker) fn github_auth_gap(mode: GithubAuthMode) -> Option<String> {
    match mode {
        GithubAuthMode::Fixture | GithubAuthMode::EnvToken | GithubAuthMode::GhCli => None,
        GithubAuthMode::MissingGh => {
            Some("GitHub Project v2 live reads require the `gh` CLI on PATH.".into())
        }
        GithubAuthMode::Unauthenticated { reason } => {
            let suffix = reason
                .filter(|message| !message.is_empty())
                .map(|message| format!(" Last auth check error: {message}"))
                .unwrap_or_default();
            Some(format!(
                "GitHub Project v2 live reads require `gh auth login` or GITHUB_TOKEN/GH_TOKEN; no usable GitHub auth was detected.{suffix}"
            ))
        }
    }
}

pub(in crate::tracker) fn run_gh_graphql(
    args: Vec<String>,
) -> Result<serde_json::Value, TrackerError> {
    GithubCliAccess::run_json(GithubCliApi::Graphql, args)
}

pub(in crate::tracker) fn run_gh_api_json(
    args: Vec<String>,
) -> Result<serde_json::Value, TrackerError> {
    GithubCliAccess::run_json(GithubCliApi::RestJson, args)
}

pub(in crate::tracker) fn project_state_error_is_retryable(error: &TrackerError) -> bool {
    matches!(
        classify_project_state_error(error),
        ProjectStateFailureKind::Network
            | ProjectStateFailureKind::TransientBackend
            | ProjectStateFailureKind::RateLimit
    )
}

fn project_state_retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(match attempt {
        0 | 1 => 250,
        2 => 1_000,
        _ => 2_000,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graphql_args(operation: &str) -> Vec<String> {
        vec![
            "api".into(),
            "graphql".into(),
            "-f".into(),
            format!("query=query {operation} {{ rateLimit {{ cost remaining resetAt }} }}"),
        ]
    }

    #[test]
    fn graphql_budget_evidence_parses_operation_and_provider_reset() {
        let response = serde_json::json!({
            "data": {
                "rateLimit": {
                    "cost": 7,
                    "remaining": 42,
                    "resetAt": "2026-08-07T20:30:00Z"
                }
            }
        });

        let evidence =
            with_github_graphql_request_context("autopilot.main.selection_refresh", None, || {
                GithubGraphqlBudgetEvidence::from_response(&graphql_args("FixturePlan"), &response)
            });

        assert_eq!(evidence.operation, "FixturePlan");
        assert_eq!(evidence.action, "autopilot.main.selection_refresh");
        assert_eq!(evidence.cost, Some(7));
        assert_eq!(evidence.remaining, Some(42));
        assert!(evidence.reset_at_ms.is_some());
        assert!(!evidence.compact().contains("token"));
    }

    #[test]
    fn graphql_reset_parser_rejects_missing_or_malformed_evidence() {
        assert!(parse_github_graphql_reset_at_ms("2026-08-07T20:30:00Z").is_some());
        assert_eq!(parse_github_graphql_reset_at_ms("soon"), None);
        assert_eq!(parse_github_graphql_reset_at_ms(""), None);
    }

    #[test]
    fn shared_cooldown_deduplicates_equivalent_lane_decisions() {
        let cooldown = GithubGraphqlCooldown::default();
        let now_ms = 1_000;
        let evidence = GithubGraphqlBudgetEvidence {
            operation: "FixturePlan".into(),
            action: "autopilot.main.selection_refresh".into(),
            cost: Some(1),
            remaining: Some(0),
            reset_at: Some("1970-01-01T00:00:03Z".into()),
            reset_at_ms: Some(3_000),
        };

        assert!(cooldown.schedule(&evidence, now_ms));
        assert!(!cooldown.schedule(&evidence, now_ms));
        assert!(!cooldown.schedule(&evidence, now_ms));

        let state = cooldown
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(state.decision_id, 1);
        assert_eq!(state.until_ms, 3_000);
    }

    #[test]
    fn shared_cooldown_uses_bounded_fallback_and_honors_cancellation() {
        let cooldown = GithubGraphqlCooldown::default();
        let now_ms = unix_time_ms();
        let evidence = GithubGraphqlBudgetEvidence {
            operation: "FixturePlan".into(),
            action: "autopilot.review.selection_refresh".into(),
            cost: None,
            remaining: None,
            reset_at: Some("malformed".into()),
            reset_at_ms: None,
        };
        assert!(cooldown.schedule(&evidence, now_ms));
        {
            let state = cooldown
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            assert_eq!(
                state.until_ms,
                now_ms.saturating_add(GITHUB_GRAPHQL_COOLDOWN_FALLBACK_MS)
            );
        }

        let cancellation = Arc::new(AtomicBool::new(true));
        let error = with_github_graphql_request_context(
            "autopilot.review.selection_refresh",
            Some(cancellation),
            || cooldown.wait().unwrap_err(),
        );
        assert!(error.to_string().contains("cooldown cancelled"));
    }
}
