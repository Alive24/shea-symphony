use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::model::{GateDecision, GateDecisionKind, TrackerIssue};

const REQUIRED_SECTIONS: &[(&str, &str)] = &[
    ("Issue Goal", "goal"),
    ("Why Now", "why now"),
    ("Issue Context", "context"),
    ("Scope", "scope"),
    ("Canonical References", "canonical references"),
    ("Target Repository / Package", "target repo or package"),
    ("Non-Negotiable Guardrails", "guardrails and constraints"),
    ("Verification", "validation requirements"),
    ("Completion Criteria", "acceptance criteria"),
];

pub fn evaluate_issue(issue: &TrackerIssue) -> GateDecision {
    let Some(description) = issue.description.as_deref() else {
        return GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing: vec!["description with executable issue contract".into()],
            assumptions: Vec::new(),
            notes: vec!["Issue body is empty.".into()],
        };
    };

    let mut missing = Vec::new();
    for (heading, label) in REQUIRED_SECTIONS {
        if !contains_heading(description, heading) {
            missing.push((*label).to_string());
        }
    }
    if let Err(missing_uat) = validate_uat_required(description) {
        missing.push(missing_uat);
    }
    if let Err(missing_dependency) = validate_dependency_semantics(issue, description) {
        missing.push(missing_dependency);
    }

    if has_explicit_blocked_decision(description) && missing.is_empty() {
        return GateDecision {
            kind: GateDecisionKind::Blocked,
            missing: vec!["blocked dependency must be resolved before dispatch".into()],
            assumptions: Vec::new(),
            notes: Vec::new(),
        };
    }

    if !missing.is_empty() {
        return GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing,
            assumptions: Vec::new(),
            notes: vec![
                "Issue contract does not yet match the Jade Symphony quality template.".into(),
            ],
        };
    }

    let assumptions = extract_assumptions(description);
    if assumptions.is_empty() {
        GateDecision::ready()
    } else {
        GateDecision {
            kind: GateDecisionKind::ReadyWithAssumptions,
            missing: Vec::new(),
            assumptions,
            notes: Vec::new(),
        }
    }
}

pub fn evaluate_issue_with_dependency_preflight(
    issue: &TrackerIssue,
    terminal_states: &std::collections::BTreeSet<String>,
) -> GateDecision {
    let decision = evaluate_issue(issue);
    if !decision.is_dispatchable() {
        return decision;
    }

    if let Some(blocker) = unresolved_tracker_blocker(issue, terminal_states) {
        return GateDecision {
            kind: GateDecisionKind::Blocked,
            missing: vec![format!("unresolved blocking dependency: {blocker}")],
            assumptions: decision.assumptions,
            notes: vec!["Tracker dependency preflight blocked dispatch.".into()],
        };
    }

    decision
}

pub fn evaluate_issue_with_source_alignment(
    issue: &TrackerIssue,
    repo_root: &Path,
    expected_target_repo: Option<&str>,
) -> GateDecision {
    let mut decision = evaluate_issue(issue);
    if !decision.is_dispatchable() {
        return decision;
    }

    let description = issue.description.as_deref().unwrap_or_default();
    let mut missing = Vec::new();
    let mut notes = Vec::new();

    if let Some(expected) = expected_target_repo {
        match target_repository_in_section(description) {
            Some(actual) if normalize_target_repo(&actual) == normalize_target_repo(expected) => {}
            Some(actual) => missing.push(format!(
                "target repository mismatch: expected `{expected}`, found `{actual}`"
            )),
            None => missing.push("target repository bullet".into()),
        }
    }

    for path in referenced_paths(description) {
        if !repo_root.join(&path).exists() {
            missing.push(format!("referenced path missing: `{}`", path.display()));
        }
    }

    let commands = verification_commands(description);
    if commands.is_empty() {
        missing.push("verification command".into());
    } else {
        for command in commands {
            if !is_supported_verification_command(&command) {
                missing.push(format!("unsupported verification command: `{command}`"));
            }
        }
    }

    if missing.is_empty() {
        notes.push("Source alignment preflight passed.".into());
        decision.notes.extend(notes);
        decision
    } else {
        GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing,
            assumptions: decision.assumptions,
            notes: vec![
                "Source alignment preflight found missing or unsupported repository context."
                    .into(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmGateMode {
    Disabled,
    Advisory,
    Required,
}

impl LlmGateMode {
    pub fn parse(value: &str) -> Self {
        match value {
            "advisory" => Self::Advisory,
            "required" => Self::Required,
            _ => Self::Disabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmGateOptions {
    pub mode: LlmGateMode,
    pub command: Option<String>,
    pub timeout_ms: u64,
}

pub fn evaluate_issue_with_llm_gate(
    issue: &TrackerIssue,
    deterministic: GateDecision,
    options: &LlmGateOptions,
) -> GateDecision {
    if !deterministic.is_dispatchable() {
        let mut decision = deterministic;
        if !matches!(options.mode, LlmGateMode::Disabled) {
            decision
                .notes
                .push("LLM gate skipped because deterministic gate failed.".into());
        }
        return decision;
    }

    match options.mode {
        LlmGateMode::Disabled => deterministic,
        LlmGateMode::Advisory => match run_llm_gate_command(issue, &deterministic, options) {
            Ok(report) => advisory_decision(deterministic, report),
            Err(error) => {
                let mut decision = deterministic;
                decision
                    .notes
                    .push(format!("LLM advisory gate unavailable: {error}"));
                decision
            }
        },
        LlmGateMode::Required => match run_llm_gate_command(issue, &deterministic, options) {
            Ok(report) => report.into_gate_decision(),
            Err(error) => GateDecision {
                kind: GateDecisionKind::NeedToClarify,
                missing: vec!["required LLM quality gate result".into()],
                assumptions: deterministic.assumptions,
                notes: vec![format!("Required LLM quality gate failed: {error}")],
            },
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LlmGateReport {
    kind: GateDecisionKind,
    missing: Vec<String>,
    assumptions: Vec<String>,
    notes: Vec<String>,
}

impl LlmGateReport {
    fn into_gate_decision(self) -> GateDecision {
        GateDecision {
            kind: self.kind,
            missing: self.missing,
            assumptions: self.assumptions,
            notes: self.notes,
        }
    }
}

fn advisory_decision(mut deterministic: GateDecision, report: LlmGateReport) -> GateDecision {
    deterministic
        .notes
        .push(format!("LLM advisory decision: {:?}", report.kind));
    deterministic.notes.extend(
        report
            .missing
            .into_iter()
            .map(|item| format!("LLM finding: {item}")),
    );
    deterministic.assumptions.extend(
        report
            .assumptions
            .into_iter()
            .map(|item| format!("LLM: {item}")),
    );
    deterministic.notes.extend(report.notes);
    deterministic
}

fn run_llm_gate_command(
    issue: &TrackerIssue,
    deterministic: &GateDecision,
    options: &LlmGateOptions,
) -> Result<LlmGateReport, String> {
    let Some(command) = options
        .command
        .as_deref()
        .filter(|command| !command.trim().is_empty())
    else {
        return Err("command not configured".into());
    };

    let request = serde_json::json!({
        "title": issue.title,
        "identifier": issue.identifier,
        "body": issue.description.as_deref().unwrap_or_default(),
        "deterministic": deterministic,
    });
    let mut child = Command::new("sh")
        .arg("-lc")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(request.to_string().as_bytes())
            .map_err(|error| error.to_string())?;
    }

    let started = Instant::now();
    let timeout = Duration::from_millis(options.timeout_ms.max(1));
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| error.to_string())?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!(
                    "command exited with status {}: {}",
                    output.status.code().unwrap_or(-1),
                    stderr.trim()
                ));
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            return parse_llm_gate_response(&stdout);
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("command timed out after {}ms", options.timeout_ms));
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn parse_llm_gate_response(raw: &str) -> Result<LlmGateReport, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("malformed JSON: {error}"))?;
    let decision = value
        .get("decision")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "missing `decision`".to_string())?;
    let kind = parse_gate_kind(decision)
        .ok_or_else(|| format!("unsupported LLM gate decision `{decision}`"))?;

    Ok(LlmGateReport {
        kind,
        missing: string_array(value.get("missing")),
        assumptions: string_array(value.get("assumptions")),
        notes: string_array(value.get("notes")),
    })
}

fn parse_gate_kind(value: &str) -> Option<GateDecisionKind> {
    match value {
        "Ready" => Some(GateDecisionKind::Ready),
        "ReadyWithAssumptions" => Some(GateDecisionKind::ReadyWithAssumptions),
        "NeedToClarify" => Some(GateDecisionKind::NeedToClarify),
        "TooBroad" => Some(GateDecisionKind::TooBroad),
        "Blocked" => Some(GateDecisionKind::Blocked),
        "DuplicateAlreadyCovered" => Some(GateDecisionKind::DuplicateAlreadyCovered),
        _ => None,
    }
}

fn string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn has_explicit_blocked_decision(markdown: &str) -> bool {
    markdown.lines().any(|line| {
        let normalized = line.trim().trim_start_matches('-').trim().to_lowercase();

        matches!(
            normalized.as_str(),
            "gate decision: blocked" | "classification: blocked" | "status: blocked"
        )
    })
}

fn validate_uat_required(markdown: &str) -> Result<(), String> {
    let Some(value) = section_lines(markdown, "Issue Setup")
        .into_iter()
        .find_map(|line| uat_required_value(&line))
    else {
        return Err("UAT Required field".into());
    };

    match value.to_ascii_lowercase().as_str() {
        "yes" | "no" => Ok(()),
        _ => Err("UAT Required field must be `Yes` or `No`".into()),
    }
}

fn validate_dependency_semantics(issue: &TrackerIssue, markdown: &str) -> Result<(), String> {
    if !issue.blocked_by.is_empty() {
        return Ok(());
    }

    let lines = section_lines(markdown, "Dependencies");
    let dependencies = lines
        .iter()
        .filter_map(|line| line.trim().strip_prefix('-'))
        .map(clean_markdown_value)
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>();

    if dependencies.is_empty() {
        return Ok(());
    }

    let joined = dependencies.join(" ").to_ascii_lowercase();
    if contains_ambiguous_dependency_marker(&joined) {
        Err("resolved dependency semantics".into())
    } else if claims_blocking_dependency_without_relationship(&joined) {
        Err("structured blocked-by relationship".into())
    } else if contains_any_dependency_marker(&joined) {
        Ok(())
    } else {
        Err("resolved dependency semantics".into())
    }
}

fn contains_any_dependency_marker(text: &str) -> bool {
    text.contains("no blocking dependenc")
        || text.contains("no known blocking dependenc")
        || text.contains("no dependenc")
        || text.contains("none")
        || text.contains("blocked by")
        || text.contains("depends on")
        || text.contains("dependency")
        || text.contains("dependencies")
        || text.contains("parallel-safe")
        || text.contains("parallel safe")
        || text.contains("overlap")
        || text.contains("supersede")
        || text.contains("pull request")
        || text.contains("pr #")
        || text.contains('#')
}

fn claims_blocking_dependency_without_relationship(text: &str) -> bool {
    (text.contains("blocked by")
        || text.contains("depends on")
        || text.contains("dependency:")
        || text.contains("dependencies:")
        || text.contains("requires #"))
        && !text.contains("no blocking")
        && !text.contains("no known blocking")
        && !text.contains("none")
}

fn contains_ambiguous_dependency_marker(text: &str) -> bool {
    text.contains("tbd")
        || text.contains("unknown dependency")
        || text.contains("unknown dependencies")
        || text.contains("dependencies unknown")
        || text.contains("dependency unknown")
        || text.contains("unclear")
        || text.contains("potential dependency")
        || text.contains("requires operator confirmation")
}

fn unresolved_tracker_blocker(
    issue: &TrackerIssue,
    terminal_states: &std::collections::BTreeSet<String>,
) -> Option<String> {
    issue.blocked_by.iter().find_map(|blocker| {
        let state = blocker.state.as_deref().map(crate::model::normalize_state);
        let resolved = state
            .as_ref()
            .map(|state| terminal_states.contains(state))
            .unwrap_or(false);
        (!resolved).then(|| {
            blocker
                .identifier
                .clone()
                .or_else(|| blocker.id.clone())
                .unwrap_or_else(|| "unknown blocker".into())
        })
    })
}

fn uat_required_value(line: &str) -> Option<String> {
    let line = line.trim().trim_start_matches('-').trim();
    let (label, value) = line.split_once(':')?;
    if label.trim().eq_ignore_ascii_case("UAT Required") {
        Some(clean_markdown_value(value))
    } else {
        None
    }
}

fn contains_heading(markdown: &str, heading: &str) -> bool {
    markdown.lines().any(|line| {
        let line = line.trim_start_matches('#').trim();
        line.eq_ignore_ascii_case(heading)
    })
}

fn extract_assumptions(markdown: &str) -> Vec<String> {
    let mut in_assumptions = false;
    let mut assumptions = Vec::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        let heading = trimmed.trim_start_matches('#').trim();

        if heading.eq_ignore_ascii_case("Assumptions") {
            in_assumptions = true;
            continue;
        }

        if in_assumptions && trimmed.starts_with('#') {
            break;
        }

        if in_assumptions && trimmed.starts_with('-') {
            assumptions.push(trimmed.trim_start_matches('-').trim().to_string());
        }
    }

    assumptions
}

fn first_bullet_in_section(markdown: &str, heading: &str) -> Option<String> {
    section_lines(markdown, heading)
        .into_iter()
        .find_map(|line| {
            line.trim()
                .strip_prefix('-')
                .map(clean_markdown_value)
                .filter(|value| !value.is_empty())
        })
}

fn target_repository_in_section(markdown: &str) -> Option<String> {
    first_bullet_in_section(markdown, "Target Repository / Package").map(|value| {
        let Some((label, repository)) = value.split_once(':') else {
            return value;
        };
        let normalized_label = label.trim().to_ascii_lowercase();
        if matches!(
            normalized_label.as_str(),
            "repository" | "repo" | "target repository"
        ) {
            clean_markdown_value(repository)
        } else {
            value
        }
    })
}

fn referenced_paths(markdown: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for heading in ["Relevant Knowledge Sources", "Relevant Code Paths"] {
        for line in section_lines(markdown, heading) {
            let Some(raw) = line.trim().strip_prefix('-') else {
                continue;
            };
            let value = clean_markdown_value(raw);
            if is_local_reference(&value) {
                paths.push(PathBuf::from(value));
            }
        }
    }
    paths
}

fn verification_commands(markdown: &str) -> Vec<String> {
    section_lines(markdown, "Functional Verification")
        .into_iter()
        .flat_map(|line| {
            let Some(raw) = line.trim().strip_prefix('-') else {
                return Vec::new();
            };
            let value = clean_markdown_value(raw);
            if looks_like_command(&value) {
                vec![value]
            } else if is_standard_rust_verification_phrase(&value) {
                standard_rust_verification_commands()
            } else {
                Vec::new()
            }
        })
        .collect()
}

fn section_lines(markdown: &str, heading: &str) -> Vec<String> {
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        let normalized_heading = trimmed.trim_start_matches('#').trim();
        if normalized_heading.eq_ignore_ascii_case(heading) {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with('#') {
            break;
        }
        if in_section {
            lines.push(line.to_string());
        }
    }
    lines
}

fn clean_markdown_value(raw: &str) -> String {
    let value = raw.trim();
    let value = value
        .strip_prefix("[ ]")
        .or_else(|| value.strip_prefix("[x]"))
        .or_else(|| value.strip_prefix("[X]"))
        .unwrap_or(value);
    value
        .trim()
        .trim_matches('`')
        .trim()
        .trim_end_matches('.')
        .to_string()
}

fn normalize_target_repo(value: &str) -> String {
    clean_markdown_value(value)
        .trim_start_matches("https://github.com/")
        .trim_start_matches("github.com/")
        .trim_matches('`')
        .to_ascii_lowercase()
}

fn is_local_reference(value: &str) -> bool {
    !value.starts_with("http://")
        && !value.starts_with("https://")
        && !value.starts_with('$')
        && (value.contains('/')
            || value.starts_with("Cargo.")
            || value == "README.md"
            || value.ends_with(".rs")
            || value.ends_with(".md"))
}

fn looks_like_command(value: &str) -> bool {
    matches!(
        value.split_whitespace().next(),
        Some("cargo" | "gh" | "git" | "pnpm" | "npm" | "node")
    )
}

fn is_standard_rust_verification_phrase(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    normalized.contains("standard rust verification suite")
        || normalized.contains("standard rust verification")
}

fn standard_rust_verification_commands() -> Vec<String> {
    vec![
        "cargo test".into(),
        "cargo fmt --check".into(),
        "cargo clippy --all-targets --all-features -- -D warnings".into(),
    ]
}

fn is_supported_verification_command(command: &str) -> bool {
    let parts: Vec<_> = command.split_whitespace().collect();
    matches!(
        parts.as_slice(),
        ["cargo", "test"]
            | ["cargo", "fmt", "--check"]
            | ["cargo", "clippy", ..]
            | ["cargo", "run", ..]
            | ["gh", ..]
            | ["git", ..]
            | ["pnpm", ..]
            | ["npm", ..]
            | ["node", ..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrackerIssue;

    fn issue(description: Option<String>) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: "1".into(),
            item_id: None,
            identifier: "#1".into(),
            title: "Implement".into(),
            description,
            url: None,
            state: "Todo".into(),
            labels: vec![],
            assignees: vec![],
            priority: None,
            branch_name: None,
            linked_pull_requests: vec![],
            blocked_by: vec![],
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn missing_contract_needs_clarification() {
        let decision = evaluate_issue(&issue(Some("thin body".into())));
        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision.missing.iter().any(|item| item == "goal"));
    }

    #[test]
    fn template_shaped_issue_is_ready() {
        let body = [
            "## Issue Setup",
            "- UAT Required: No",
            "## Issue Goal",
            "Ship a thing.",
            "## Why Now",
            "It blocks the next slice.",
            "## Issue Context",
            "Context.",
            "## Dependencies",
            "- No blocking dependencies.",
            "## Non-Negotiable Guardrails",
            "- Keep tracker abstraction.",
            "## Scope",
            "### In Scope",
            "- Code.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/jade-symphony",
            "## Verification",
            "### Completion Criteria",
            "- Tests pass.",
        ]
        .join("\n");

        assert!(evaluate_issue(&issue(Some(body))).is_dispatchable());
    }

    #[test]
    fn uat_required_accepts_yes_or_no_values() {
        for value in ["Yes", "No", "yes", "no"] {
            let body = aligned_body_with_uat(value);
            assert!(
                evaluate_issue(&issue(Some(body))).is_dispatchable(),
                "expected UAT Required: {value} to pass"
            );
        }
    }

    #[test]
    fn source_alignment_accepts_checkbox_verification_commands() {
        let body = aligned_body(
            "Alive24/jade-symphony",
            &["README.md"],
            &["src/main.rs"],
            &[
                "cargo test",
                "cargo fmt --check",
                "cargo clippy --all-targets --all-features -- -D warnings",
            ],
        )
        .replace("- `cargo test`", "- [ ] `cargo test`")
        .replace("- `cargo fmt --check`", "- [ ] `cargo fmt --check`")
        .replace(
            "- `cargo clippy --all-targets --all-features -- -D warnings`",
            "- [ ] `cargo clippy --all-targets --all-features -- -D warnings`",
        );

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            Path::new(env!("CARGO_MANIFEST_DIR")),
            Some("Alive24/jade-symphony"),
        );

        assert!(decision.is_dispatchable(), "{decision:?}");
    }

    #[test]
    fn uat_required_missing_or_malformed_needs_clarification() {
        let missing = evaluate_issue(&issue(Some(aligned_body_without_uat())));
        assert_eq!(missing.kind, GateDecisionKind::NeedToClarify);
        assert!(missing
            .missing
            .iter()
            .any(|item| item == "UAT Required field"));

        let malformed = evaluate_issue(&issue(Some(aligned_body_with_uat("Maybe"))));
        assert_eq!(malformed.kind, GateDecisionKind::NeedToClarify);
        assert!(malformed
            .missing
            .iter()
            .any(|item| item == "UAT Required field must be `Yes` or `No`"));
    }

    #[test]
    fn incidental_blocked_word_does_not_block_ready_issue() {
        let body = [
            "## Issue Setup",
            "- UAT Required: No",
            "## Issue Goal",
            "Ship a thing.",
            "## Why Now",
            "It is needed before blocked downstream work can proceed.",
            "## Issue Context",
            "Context.",
            "## Dependencies",
            "- No blocking dependencies.",
            "## Non-Negotiable Guardrails",
            "- Guard.",
            "## Scope",
            "### In Scope",
            "- Code.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/jade-symphony",
            "## Verification",
            "### Completion Criteria",
            "- Tests pass.",
        ]
        .join("\n");

        assert!(evaluate_issue(&issue(Some(body))).is_dispatchable());
    }

    #[test]
    fn explicit_blocked_decision_blocks_issue() {
        let body = [
            "## Issue Setup",
            "- UAT Required: No",
            "## Issue Goal",
            "Ship a thing.",
            "## Why Now",
            "Now.",
            "## Issue Context",
            "Context.",
            "## Dependencies",
            "- No blocking dependencies.",
            "## Non-Negotiable Guardrails",
            "- Guard.",
            "## Scope",
            "### In Scope",
            "- Code.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/jade-symphony",
            "## Verification",
            "### Completion Criteria",
            "- Tests pass.",
            "- Gate Decision: Blocked",
        ]
        .join("\n");

        let decision = evaluate_issue(&issue(Some(body)));
        assert_eq!(decision.kind, GateDecisionKind::Blocked);
    }

    #[test]
    fn missing_dependency_section_does_not_block_independent_issue() {
        let body = [
            "## Issue Setup",
            "- UAT Required: No",
            "## Issue Goal",
            "Ship a thing.",
            "## Why Now",
            "Now.",
            "## Issue Context",
            "Context.",
            "## Non-Negotiable Guardrails",
            "- Guard.",
            "## Scope",
            "### In Scope",
            "- Code.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/jade-symphony",
            "## Verification",
            "### Completion Criteria",
            "- Tests pass.",
        ]
        .join("\n");

        let decision = evaluate_issue(&issue(Some(body)));

        assert!(decision.is_dispatchable(), "{decision:?}");
    }

    #[test]
    fn body_only_blocker_claim_needs_structured_relationship() {
        let mut body = aligned_body_with_uat("No");
        body = body.replace(
            "## Dependencies\n- No blocking dependencies.",
            "## Dependencies\n\n- Blocked by #44 until it is Done.",
        );

        let decision = evaluate_issue(&issue(Some(body)));

        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision
            .missing
            .contains(&"structured blocked-by relationship".to_string()));
    }

    #[test]
    fn ambiguous_dependency_semantics_needs_clarification() {
        let mut body = aligned_body_with_uat("No");
        body = body.replace(
            "## Dependencies\n- No blocking dependencies.",
            "## Dependencies\n\n- Potential dependency requires operator confirmation: maybe #44.",
        );

        let decision = evaluate_issue(&issue(Some(body)));

        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision
            .missing
            .contains(&"resolved dependency semantics".to_string()));
    }

    #[test]
    fn dependency_preflight_blocks_non_terminal_tracker_blocker() {
        let body = aligned_body_with_uat("No").replace(
            "## Dependencies\n- No blocking dependencies.",
            "## Dependencies\n",
        );
        let mut issue = issue(Some(body));
        issue.blocked_by.push(crate::model::BlockerRef {
            id: None,
            identifier: Some("#99".into()),
            state: Some("In Progress".into()),
        });
        let terminal_states = std::collections::BTreeSet::from(["done".to_string()]);

        let decision = evaluate_issue_with_dependency_preflight(&issue, &terminal_states);

        assert_eq!(decision.kind, GateDecisionKind::Blocked);
        assert!(decision.missing[0].contains("#99"));
    }

    #[test]
    fn dependency_preflight_allows_terminal_tracker_blocker() {
        let body = aligned_body_with_uat("No").replace(
            "## Dependencies\n- No blocking dependencies.",
            "## Dependencies\n",
        );
        let mut issue = issue(Some(body));
        issue.blocked_by.push(crate::model::BlockerRef {
            id: None,
            identifier: Some("#99".into()),
            state: Some("Done".into()),
        });
        let terminal_states = std::collections::BTreeSet::from(["done".to_string()]);

        let decision = evaluate_issue_with_dependency_preflight(&issue, &terminal_states);

        assert!(decision.is_dispatchable(), "{decision:?}");
    }

    #[test]
    fn source_alignment_accepts_existing_paths_and_supported_commands() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/main.rs"), "").unwrap();
        std::fs::create_dir_all(temp.path().join("docs")).unwrap();
        std::fs::write(temp.path().join("docs/dogfood-readiness.md"), "").unwrap();
        let body = aligned_body(
            "Alive24/jade-symphony",
            &["docs/dogfood-readiness.md"],
            &["src/main.rs"],
            &["cargo test", "cargo fmt --check"],
        );

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            temp.path(),
            Some("Alive24/jade-symphony"),
        );

        assert!(decision.is_dispatchable());
        assert!(decision
            .notes
            .contains(&"Source alignment preflight passed.".to_string()));
    }

    #[test]
    fn source_alignment_accepts_labeled_target_repository_bullet() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/quality_gate.rs"), "").unwrap();
        let body = aligned_body_with_target_line(
            "- Repository: `Alive24/jade-symphony`",
            &[],
            &["src/quality_gate.rs"],
            &["cargo test"],
        );

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            temp.path(),
            Some("Alive24/jade-symphony"),
        );

        assert!(decision.is_dispatchable(), "{decision:?}");
    }

    #[test]
    fn source_alignment_accepts_standard_rust_verification_suite_wording() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/quality_gate.rs"), "").unwrap();
        let body = aligned_body_with_target_line(
            "- Repository: `Alive24/jade-symphony`",
            &[],
            &["src/quality_gate.rs"],
            &["Run the standard Rust verification suite."],
        );

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            temp.path(),
            Some("Alive24/jade-symphony"),
        );

        assert!(decision.is_dispatchable(), "{decision:?}");
    }

    #[test]
    fn source_alignment_reports_missing_paths() {
        let temp = tempfile::tempdir().unwrap();
        let body = aligned_body(
            "Alive24/jade-symphony",
            &["docs/missing.md"],
            &["src/main.rs"],
            &["cargo test"],
        );

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            temp.path(),
            Some("Alive24/jade-symphony"),
        );

        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision
            .missing
            .iter()
            .any(|item| item.contains("referenced path missing")));
    }

    #[test]
    fn source_alignment_reports_target_repo_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let body = aligned_body("Other/repo", &[], &[], &["cargo test"]);

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            temp.path(),
            Some("Alive24/jade-symphony"),
        );

        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision
            .missing
            .iter()
            .any(|item| item.contains("target repository mismatch")));
    }

    #[test]
    fn source_alignment_reports_weak_verification() {
        let temp = tempfile::tempdir().unwrap();
        let body = aligned_body("Alive24/jade-symphony", &[], &[], &["manually inspect"]);

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            temp.path(),
            Some("Alive24/jade-symphony"),
        );

        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision
            .missing
            .iter()
            .any(|item| item == "verification command"));
    }

    #[test]
    fn required_llm_gate_can_pass_with_structured_output() {
        let deterministic = GateDecision::ready();
        let decision = evaluate_issue_with_llm_gate(
            &issue(Some(aligned_body(
                "Alive24/jade-symphony",
                &[],
                &[],
                &["cargo test"],
            ))),
            deterministic,
            &LlmGateOptions {
                mode: LlmGateMode::Required,
                command: Some("sh examples/fixtures/llm-gate-ready.sh".into()),
                timeout_ms: 5_000,
            },
        );

        assert_eq!(decision.kind, GateDecisionKind::ReadyWithAssumptions);
        assert!(decision
            .assumptions
            .contains(&"LLM fixture says scope is coherent".to_string()));
    }

    #[test]
    fn required_llm_gate_blocks_on_malformed_output() {
        let decision = evaluate_issue_with_llm_gate(
            &issue(Some(aligned_body(
                "Alive24/jade-symphony",
                &[],
                &[],
                &["cargo test"],
            ))),
            GateDecision::ready(),
            &LlmGateOptions {
                mode: LlmGateMode::Required,
                command: Some("sh examples/fixtures/llm-gate-malformed.sh".into()),
                timeout_ms: 5_000,
            },
        );

        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision
            .notes
            .iter()
            .any(|note| note.contains("Required LLM quality gate failed")));
    }

    #[test]
    fn advisory_llm_gate_records_finding_without_blocking() {
        let decision = evaluate_issue_with_llm_gate(
            &issue(Some(aligned_body(
                "Alive24/jade-symphony",
                &[],
                &[],
                &["cargo test"],
            ))),
            GateDecision::ready(),
            &LlmGateOptions {
                mode: LlmGateMode::Advisory,
                command: Some("sh examples/fixtures/llm-gate-clarify.sh".into()),
                timeout_ms: 5_000,
            },
        );

        assert_eq!(decision.kind, GateDecisionKind::Ready);
        assert!(decision
            .notes
            .iter()
            .any(|note| note.contains("LLM advisory decision: NeedToClarify")));
    }

    #[test]
    fn deterministic_failure_precedes_llm_gate() {
        let deterministic = GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing: vec!["verification command".into()],
            assumptions: Vec::new(),
            notes: Vec::new(),
        };
        let decision = evaluate_issue_with_llm_gate(
            &issue(Some("thin".into())),
            deterministic,
            &LlmGateOptions {
                mode: LlmGateMode::Required,
                command: Some("sh examples/fixtures/llm-gate-ready.sh".into()),
                timeout_ms: 5_000,
            },
        );

        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision
            .notes
            .contains(&"LLM gate skipped because deterministic gate failed.".to_string()));
    }

    fn aligned_body(target_repo: &str, docs: &[&str], paths: &[&str], commands: &[&str]) -> String {
        aligned_body_with_target_line(&format!("- `{target_repo}`"), docs, paths, commands)
    }

    fn aligned_body_with_target_line(
        target_line: &str,
        docs: &[&str],
        paths: &[&str],
        commands: &[&str],
    ) -> String {
        aligned_body_with_target_line_and_uat(target_line, Some("No"), docs, paths, commands)
    }

    fn aligned_body_with_uat(uat: &str) -> String {
        aligned_body_with_target_line_and_uat(
            "- `Alive24/jade-symphony`",
            Some(uat),
            &[],
            &[],
            &["cargo test"],
        )
    }

    fn aligned_body_without_uat() -> String {
        aligned_body_with_target_line_and_uat(
            "- `Alive24/jade-symphony`",
            None,
            &[],
            &[],
            &["cargo test"],
        )
    }

    fn aligned_body_with_target_line_and_uat(
        target_line: &str,
        uat: Option<&str>,
        docs: &[&str],
        paths: &[&str],
        commands: &[&str],
    ) -> String {
        let docs = docs
            .iter()
            .map(|path| format!("- `{path}`"))
            .collect::<Vec<_>>()
            .join("\n");
        let paths = paths
            .iter()
            .map(|path| format!("- `{path}`"))
            .collect::<Vec<_>>()
            .join("\n");
        let commands = commands
            .iter()
            .map(|command| format!("- `{command}`"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut lines = vec!["## Issue Setup".to_string()];
        if let Some(uat) = uat {
            lines.push(format!("- UAT Required: {uat}"));
        }
        lines.extend(
            [
                "## Issue Goal",
                "Ship a thing.",
                "## Why Now",
                "Now.",
                "## Issue Context",
                "Context.",
                "## Dependencies",
                "- No blocking dependencies.",
                "## Decisions / Assumptions",
                "### Assumptions",
                "- Deterministic source checks are enough.",
                "## Non-Negotiable Guardrails",
                "- Guard.",
                "## Scope",
                "### In Scope",
                "- Code.",
                "## Canonical References",
                "### Target Repository / Package",
                target_line,
                "### Relevant Knowledge Sources",
                &docs,
                "### Relevant Code Paths",
                &paths,
                "## Verification",
                "### Completion Criteria",
                "- Pass.",
                "### Functional Verification",
                &commands,
            ]
            .into_iter()
            .map(ToString::to_string),
        );
        lines.join("\n")
    }
}
