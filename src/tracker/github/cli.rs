use std::fs;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
                        thread::sleep(project_state_retry_delay(attempt));
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
        let output = run_gh_command(args, api.operation_label())?;

        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let kind = classify_project_state_failure_message(&message);
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
        api.validate_response(&response)?;
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
