use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::agent::message_field;
use crate::model::AgentEvent;

use super::{AgentReviewReport, ReviewFinding, ReviewFindingClass};

pub(super) fn review_output_schema() -> serde_json::Value {
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

pub(super) fn structured_report(
    raw: &str,
    backend_name: &str,
    display_name: &str,
    session_id: Option<String>,
    stderr: String,
    exit_status: Option<String>,
) -> Result<AgentReviewReport, String> {
    let parsed: StructuredReview = serde_json::from_str(raw.trim())
        .map_err(|error| format!("{display_name} returned malformed structured output: {error}"))?;
    if parsed.summary.trim().is_empty() {
        return Err(format!(
            "{display_name} structured output has an empty summary"
        ));
    }

    let mut findings = Vec::with_capacity(parsed.findings.len());
    for finding in parsed.findings {
        if finding.title.trim().is_empty()
            || finding.body.trim().is_empty()
            || finding.evidence.trim().is_empty()
        {
            return Err(format!(
                "{display_name} structured finding is missing title, body, or evidence"
            ));
        }
        if !matches!(
            finding.severity.as_str(),
            "critical" | "high" | "medium" | "low" | "note"
        ) {
            return Err(format!(
                "{display_name} structured finding has unsupported severity `{}`",
                finding.severity
            ));
        }
        if finding
            .file
            .as_deref()
            .is_some_and(|file| file.trim().is_empty())
        {
            return Err(format!(
                "{display_name} structured finding has an empty file"
            ));
        }
        if finding.line == Some(0) {
            return Err(format!(
                "{display_name} structured finding line must be one-based"
            ));
        }
        if finding.line.is_some()
            && finding
                .file
                .as_deref()
                .is_none_or(|file| file.trim().is_empty())
        {
            return Err(format!(
                "{display_name} structured finding has a line without a file"
            ));
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
            return Err(format!(
                "{display_name} pass classification conflicts with blocking findings"
            ));
        }
        StructuredTerminal::Rework if !has_confirmed => {
            return Err(format!(
                "{display_name} rework classification has no confirmed finding"
            ));
        }
        StructuredTerminal::NeedsContext if !has_needs_context => {
            return Err(format!(
                "{display_name} needs_context classification has no Needs Context finding"
            ));
        }
        StructuredTerminal::NeedsContext if has_confirmed => {
            return Err(format!(
                "{display_name} needs_context classification conflicts with a confirmed finding"
            ));
        }
        _ => {}
    }

    Ok(AgentReviewReport {
        reviewer_backend: backend_name.into(),
        findings,
        summary: Some(parsed.summary),
        stdout: Some(raw.to_string()),
        stderr: Some(stderr),
        exit_status,
        session_id,
    })
}

pub(super) fn workspace_state(workspace: &Path) -> Result<Vec<u8>, String> {
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

pub(super) fn artifact_paths(attempts: &[Vec<AgentEvent>], field: &str) -> Vec<String> {
    attempts
        .iter()
        .filter_map(|events| message_field(events, field))
        .collect()
}

pub(super) fn artifact_texts(attempts: &[Vec<AgentEvent>], field: &str) -> String {
    artifact_paths(attempts, field)
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn redact_command_preview(command: &str) -> String {
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

pub(super) fn session_id(events: &[AgentEvent]) -> Option<String> {
    events.iter().find_map(|event| match event {
        AgentEvent::SessionStarted { session_id, .. } => Some(session_id.clone()),
        _ => None,
    })
}

pub(super) fn completed_summary(events: &[AgentEvent]) -> Option<String> {
    events.iter().rev().find_map(|event| match event {
        AgentEvent::Completed { summary, .. } => Some(summary.clone()),
        _ => None,
    })
}

pub(super) fn terminal_error(events: &[AgentEvent]) -> Option<String> {
    events.iter().find_map(|event| match event {
        AgentEvent::Failed { error, .. } => Some(error.clone()),
        _ => None,
    })
}

pub(super) fn executable_path(command: &str) -> Option<PathBuf> {
    for token in command.split_whitespace() {
        let token = token.trim_matches(|character| matches!(character, '\'' | '"' | ';'));
        if token == "env" || token.contains('=') && !token.starts_with('/') {
            continue;
        }
        let executable = PathBuf::from(token);
        if executable.is_absolute() || token.contains('/') {
            return Some(executable);
        }
        return std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(token))
                .find(|candidate| candidate.is_file())
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::structured_report;

    fn assert_invalid(raw: &str, expected: &str) {
        let error = structured_report(
            raw,
            "claude-code",
            "Claude Review",
            Some("session-1".into()),
            String::new(),
            Some("0".into()),
        )
        .unwrap_err();
        assert!(error.contains(expected), "{error}");
    }

    #[test]
    fn schema_incomplete_unsupported_and_contradictory_reports_fail_closed() {
        assert_invalid(
            r#"{"summary":"Missing findings","terminal_classification":"pass"}"#,
            "missing field",
        );
        assert_invalid(
            r#"{"summary":"Incomplete finding","terminal_classification":"rework","findings":[{"class":"confirmed","severity":"high","title":"Defect","body":"Body","file":"src/lib.rs","line":7}]}"#,
            "missing field",
        );
        assert_invalid(
            r#"{"summary":"Unsupported severity","terminal_classification":"rework","findings":[{"class":"confirmed","severity":"urgent","title":"Defect","body":"Body","file":"src/lib.rs","line":7,"evidence":"Evidence"}]}"#,
            "unsupported severity",
        );
        assert_invalid(
            r#"{"summary":"Contradictory pass","terminal_classification":"pass","findings":[{"class":"confirmed","severity":"high","title":"Defect","body":"Body","file":"src/lib.rs","line":7,"evidence":"Evidence"}]}"#,
            "pass classification conflicts",
        );
        assert_invalid(
            r#"{"summary":"Contradictory rework","terminal_classification":"rework","findings":[]}"#,
            "rework classification has no confirmed finding",
        );
        assert_invalid(
            r#"{"summary":"Contradictory context","terminal_classification":"needs_context","findings":[{"class":"confirmed","severity":"high","title":"Defect","body":"Body","file":"src/lib.rs","line":7,"evidence":"Evidence"},{"class":"needs_context","severity":"note","title":"Context","body":"Body","file":null,"line":null,"evidence":"Evidence"}]}"#,
            "needs_context classification conflicts",
        );
    }
}
