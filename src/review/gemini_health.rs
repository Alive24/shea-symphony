use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::classify_usage_limit_text;

use super::{ReviewJob, ReviewJobState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeminiReviewHealthCategory {
    QuotaRateLimit,
    TransientBackend,
    NonRecoveringConfig,
    NonRecoveringPolicy,
}

impl GeminiReviewHealthCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QuotaRateLimit => "quota_rate_limit",
            Self::TransientBackend => "transient_backend",
            Self::NonRecoveringConfig => "non_recovering_config",
            Self::NonRecoveringPolicy => "non_recovering_policy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeminiReviewRecoveryPolicy {
    WaitAndRetry,
    RetryWithBackoff,
    RequiresHumanInput,
}

impl GeminiReviewRecoveryPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WaitAndRetry => "wait_and_retry",
            Self::RetryWithBackoff => "retry_with_backoff",
            Self::RequiresHumanInput => "requires_human_input",
        }
    }

    pub fn is_recoverable(self) -> bool {
        matches!(self, Self::WaitAndRetry | Self::RetryWithBackoff)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeminiReviewHealthDiagnostic {
    pub category: GeminiReviewHealthCategory,
    pub recovery_policy: GeminiReviewRecoveryPolicy,
    pub reason_code: String,
    pub message: String,
    pub operator_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

impl GeminiReviewHealthDiagnostic {
    pub fn signature(&self) -> String {
        format!("{}:{}", self.category.as_str(), self.reason_code)
    }

    pub fn is_recoverable(&self) -> bool {
        self.recovery_policy.is_recoverable()
    }

    pub fn to_error_message(&self) -> String {
        let retry_after = self
            .retry_after_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".into());
        format!(
            "Review backend health check classified category={} reason={} recovery_policy={} retry_after_ms={}: {}",
            self.category.as_str(),
            self.reason_code,
            self.recovery_policy.as_str(),
            retry_after,
            self.message
        )
    }
}

pub(super) fn diagnose_gemini_spawn_failure(command: &str, error: &std::io::Error) -> String {
    diagnose_backend_spawn_failure("Gemini", "review_lane.gemini_command", command, error)
}

pub(super) fn diagnose_agy_spawn_failure(command: &str, error: &std::io::Error) -> String {
    diagnose_backend_spawn_failure("agy", "review_lane.agy_command", command, error)
}

fn diagnose_backend_spawn_failure(
    backend_label: &str,
    command_field: &str,
    command: &str,
    error: &std::io::Error,
) -> String {
    match error.kind() {
        ErrorKind::NotFound if command_uses_path_lookup(command) => format!(
            "review backend startup failed: configured command: `{command}`; resolved executable: not found in worker PATH; suggested fix: configure `{command_field}` with an absolute {backend_label} command path, or export a worker PATH that can resolve `{command}`; retry: rerun `review loop` after updating the workflow or environment."
        ),
        ErrorKind::NotFound => format!(
            "review backend startup failed: configured command: `{command}`; resolved executable: path was not found or could not be executed; suggested fix: verify the configured {backend_label} path exists and is executable; retry: rerun `review loop` after updating the workflow or environment."
        ),
        ErrorKind::PermissionDenied => format!(
            "review backend startup failed: configured command: `{command}`; resolved executable: permission denied; suggested fix: make the {backend_label} command executable or configure `{command_field}` to an executable path; retry: rerun `review loop` after fixing permissions."
        ),
        _ => format!(
            "review backend startup failed: configured command: `{command}`; spawn error: {error}; suggested fix: inspect the {backend_label} CLI installation, auth/configuration, and worker environment; retry: rerun `review loop` after fixing the backend."
        ),
    }
}

fn command_uses_path_lookup(command: &str) -> bool {
    let path = Path::new(command);
    !path.is_absolute() && !command.contains(std::path::MAIN_SEPARATOR)
}

pub fn gemini_prelaunch_health_diagnostic(
    command: &str,
    model: Option<&str>,
    allowed_tools: &[String],
) -> Option<GeminiReviewHealthDiagnostic> {
    review_prelaunch_health_diagnostic(
        "Gemini",
        "review_lane.gemini_command",
        command,
        model,
        allowed_tools,
    )
}

pub fn agy_prelaunch_health_diagnostic(
    command: &str,
    model: Option<&str>,
) -> Option<GeminiReviewHealthDiagnostic> {
    review_prelaunch_health_diagnostic("agy", "review_lane.agy_command", command, model, &[])
}

fn review_prelaunch_health_diagnostic(
    backend_label: &str,
    command_field: &str,
    command: &str,
    model: Option<&str>,
    allowed_tools: &[String],
) -> Option<GeminiReviewHealthDiagnostic> {
    let command = command.trim();
    if command.is_empty() {
        return Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::NonRecoveringConfig,
            recovery_policy: GeminiReviewRecoveryPolicy::RequiresHumanInput,
            reason_code: "missing_command".into(),
            message: format!("{backend_label} review command is empty before launch."),
            operator_status: "Blocked on review backend configuration.".into(),
            retry_after_ms: None,
        });
    }

    if let Some(model) = model.map(str::trim) {
        if model.is_empty() {
            return Some(GeminiReviewHealthDiagnostic {
                category: GeminiReviewHealthCategory::NonRecoveringConfig,
                recovery_policy: GeminiReviewRecoveryPolicy::RequiresHumanInput,
                reason_code: "empty_model".into(),
                message: format!("{backend_label} review model is configured as an empty string."),
                operator_status: "Blocked on review backend model configuration.".into(),
                retry_after_ms: None,
            });
        }
    }

    if allowed_tools.iter().any(|tool| tool.trim().is_empty()) {
        return Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::NonRecoveringPolicy,
            recovery_policy: GeminiReviewRecoveryPolicy::RequiresHumanInput,
            reason_code: "empty_allowed_tool".into(),
            message: format!(
                "{backend_label} review allowed-tools configuration contains an empty tool name."
            ),
            operator_status: "Blocked on review allowed-tools configuration.".into(),
            retry_after_ms: None,
        });
    }

    let resolved = if command_uses_path_lookup(command) {
        find_executable_in_path(command)
    } else {
        Some(PathBuf::from(command))
    };
    let Some(path) = resolved else {
        return Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::NonRecoveringConfig,
            recovery_policy: GeminiReviewRecoveryPolicy::RequiresHumanInput,
            reason_code: "command_not_found".into(),
            message: format!(
                "{backend_label} review command `{command}` was not found before launch."
            ),
            operator_status: format!("Blocked until `{command_field}` or worker PATH is fixed."),
            retry_after_ms: None,
        });
    };

    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() && is_executable(&metadata) => None,
        Ok(metadata) if !metadata.is_file() => Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::NonRecoveringConfig,
            recovery_policy: GeminiReviewRecoveryPolicy::RequiresHumanInput,
            reason_code: "command_not_file".into(),
            message: format!(
                "{} review command `{}` resolves to `{}`, which is not a file.",
                backend_label,
                command,
                path.display()
            ),
            operator_status: format!(
                "Blocked until `{command_field}` points at an executable file."
            ),
            retry_after_ms: None,
        }),
        Ok(_) => Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::NonRecoveringConfig,
            recovery_policy: GeminiReviewRecoveryPolicy::RequiresHumanInput,
            reason_code: "command_not_executable".into(),
            message: format!(
                "{} review command `{}` resolves to `{}` but is not executable.",
                backend_label,
                command,
                path.display()
            ),
            operator_status: format!(
                "Blocked until `{command_field}` is made executable or reconfigured."
            ),
            retry_after_ms: None,
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::NonRecoveringConfig,
            recovery_policy: GeminiReviewRecoveryPolicy::RequiresHumanInput,
            reason_code: "command_not_found".into(),
            message: format!(
                "{} review command `{}` resolves to `{}` but the path does not exist.",
                backend_label,
                command,
                path.display()
            ),
            operator_status: format!("Blocked until `{command_field}` or worker PATH is fixed."),
            retry_after_ms: None,
        }),
        Err(error) => Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::NonRecoveringConfig,
            recovery_policy: GeminiReviewRecoveryPolicy::RequiresHumanInput,
            reason_code: "command_metadata_error".into(),
            message: format!(
                "{} review command `{}` resolves to `{}` but could not be inspected: {}.",
                backend_label,
                command,
                path.display(),
                error
            ),
            operator_status: format!("Blocked until `{command_field}` can be inspected."),
            retry_after_ms: None,
        }),
    }
}

fn find_executable_in_path(command: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(command))
            .find(|candidate| {
                fs::metadata(candidate)
                    .map(|metadata| metadata.is_file() && is_executable(&metadata))
                    .unwrap_or(false)
            })
    })
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(metadata: &fs::Metadata) -> bool {
    metadata.is_file()
}

pub fn gemini_review_health_diagnostic(job: &ReviewJob) -> Option<GeminiReviewHealthDiagnostic> {
    let backend = job.backend.to_ascii_lowercase();
    if !backend.contains("gemini") && !backend.contains("agy") {
        return None;
    }
    if !matches!(job.state, ReviewJobState::Failed | ReviewJobState::TimedOut) {
        return None;
    }

    let text = review_failure_text(job);
    classify_gemini_review_health_text(&text, job.state == ReviewJobState::TimedOut)
}

pub fn review_failure_signature(job: &ReviewJob) -> Option<String> {
    gemini_review_health_diagnostic(job)
        .map(|diagnostic| diagnostic.signature())
        .or_else(|| {
            job.error.as_deref().and_then(|error| {
                error
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .map(|line| {
                        format!(
                            "generic:{}",
                            line.split_whitespace().collect::<Vec<_>>().join(" ")
                        )
                    })
            })
        })
}

fn review_failure_text(job: &ReviewJob) -> String {
    let mut parts = Vec::new();
    if let Some(error) = job.error.as_deref() {
        parts.push(error);
    }
    if let Some(report) = job.report.as_ref() {
        if let Some(stderr) = report.stderr.as_deref() {
            parts.push(stderr);
        }
        if let Some(stdout) = report.stdout.as_deref() {
            parts.push(stdout);
        }
        if let Some(summary) = report.summary.as_deref() {
            parts.push(summary);
        }
    }
    parts.join("\n")
}

fn classify_gemini_review_health_text(
    text: &str,
    timed_out: bool,
) -> Option<GeminiReviewHealthDiagnostic> {
    let normalized = normalize_diagnostic_text(text);
    if normalized.is_empty() && !timed_out {
        return None;
    }

    if let Some(pause) = classify_usage_limit_text(text) {
        return Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::QuotaRateLimit,
            recovery_policy: GeminiReviewRecoveryPolicy::WaitAndRetry,
            reason_code: pause.classifier,
            message: "Review backend reported quota or rate limiting.".into(),
            operator_status:
                "Waiting for quota/rate-limit recovery, then retrying review automatically.".into(),
            retry_after_ms: parse_retry_after_ms(text),
        });
    }

    if contains_any(
        &normalized,
        &[
            "allowed-tools",
            "allowed tools",
            "tool is not allowed",
            "not allowed to use",
            "policy refusal",
            "policy refused",
            "blocked by policy",
            "permission denied by policy",
            "approval mode",
        ],
    ) {
        return Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::NonRecoveringPolicy,
            recovery_policy: GeminiReviewRecoveryPolicy::RequiresHumanInput,
            reason_code: "policy_or_allowed_tools".into(),
            message: "Review backend reported a policy or allowed-tools refusal.".into(),
            operator_status:
                "Blocked until review policy or allowed-tools configuration is changed.".into(),
            retry_after_ms: None,
        });
    }

    if contains_any(
        &normalized,
        &[
            "review backend startup failed",
            "not found in worker path",
            "configured command",
            "permission denied",
            "auth required",
            "authentication required",
            "not authenticated",
            "login required",
            "unknown model",
            "invalid model",
            "unsupported model",
            "model not found",
            "model is not supported",
            "model unavailable for",
            "could not inspect",
            "command `",
        ],
    ) {
        return Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::NonRecoveringConfig,
            recovery_policy: GeminiReviewRecoveryPolicy::RequiresHumanInput,
            reason_code: "command_config_or_auth".into(),
            message:
                "Review backend reported command, authentication, or model configuration failure."
                    .into(),
            operator_status:
                "Blocked until review command, auth, or model configuration is repaired.".into(),
            retry_after_ms: None,
        });
    }

    if timed_out
        || contains_any(
            &normalized,
            &[
                "temporarily unavailable",
                "service unavailable",
                "backend unavailable",
                "server unavailable",
                "overloaded",
                "capacity",
                "try again later",
                "please retry",
                "internal error",
                "deadline exceeded",
                "connection reset",
                "connection refused",
                "network error",
                "http 500",
                "http 502",
                "http 503",
                "http 504",
                " 500 ",
                " 502 ",
                " 503 ",
                " 504 ",
            ],
        )
    {
        return Some(GeminiReviewHealthDiagnostic {
            category: GeminiReviewHealthCategory::TransientBackend,
            recovery_policy: GeminiReviewRecoveryPolicy::RetryWithBackoff,
            reason_code: if timed_out {
                "timeout".into()
            } else {
                "transient_backend".into()
            },
            message: "Review backend appears temporarily unavailable or capacity-limited.".into(),
            operator_status: "Retrying with backoff while keeping review loop ownership visible."
                .into(),
            retry_after_ms: parse_retry_after_ms(text),
        });
    }

    None
}

fn normalize_diagnostic_text(text: &str) -> String {
    text.to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn contains_any(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| text.contains(pattern))
}

fn parse_retry_after_ms(text: &str) -> Option<u64> {
    let normalized = text
        .replace(['=', ':', ',', ';', '(', ')'], " ")
        .to_ascii_lowercase();
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    for (index, token) in tokens.iter().enumerate() {
        if *token == "retry-after" || (*token == "retry" && tokens.get(index + 1) == Some(&"after"))
        {
            let value_index = if *token == "retry-after" {
                index + 1
            } else {
                index + 2
            };
            if let (Some(number), Some(unit)) = (
                tokens
                    .get(value_index)
                    .and_then(|value| value.parse::<u64>().ok()),
                tokens.get(value_index + 1),
            ) {
                return Some(duration_with_unit_ms(number, unit));
            }
            if let Some(value) = parse_duration_token_ms(tokens.get(value_index).copied()) {
                return Some(value);
            }
        }
        if *token == "retry" && tokens.get(index + 1) == Some(&"in") {
            let value_index = index + 2;
            if let (Some(number), Some(unit)) = (
                tokens
                    .get(value_index)
                    .and_then(|value| value.parse::<u64>().ok()),
                tokens.get(value_index + 1),
            ) {
                return Some(duration_with_unit_ms(number, unit));
            }
            if let Some(value) = parse_duration_token_ms(tokens.get(value_index).copied()) {
                return Some(value);
            }
        }
    }
    None
}

fn parse_duration_token_ms(token: Option<&str>) -> Option<u64> {
    let token = token?;
    if let Ok(seconds) = token.parse::<u64>() {
        return Some(seconds.saturating_mul(1_000));
    }
    let split_at = token
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(token.len());
    if split_at == 0 {
        return None;
    }
    let number = token[..split_at].parse::<u64>().ok()?;
    Some(duration_with_unit_ms(number, &token[split_at..]))
}

fn duration_with_unit_ms(number: u64, unit: &str) -> u64 {
    match unit.trim_matches('.') {
        "ms" | "millisecond" | "milliseconds" => number,
        "m" | "min" | "mins" | "minute" | "minutes" => number.saturating_mul(60_000),
        "h" | "hr" | "hrs" | "hour" | "hours" => number.saturating_mul(3_600_000),
        _ => number.saturating_mul(1_000),
    }
}
