//! Backend-neutral review reports and legacy text-output classification.
#![deny(missing_docs)]

use serde::{Deserialize, Serialize};

/// Evidence classification used by every Review backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewFindingClass {
    /// Verified defect that blocks progress until Main-lane rework.
    Confirmed,
    /// Credible concern that was not independently confirmed.
    Plausible,
    /// Considered concern that review evidence disproved.
    Rejected,
    /// Missing or ambiguous evidence that prevents a conclusive review.
    NeedsContext,
}

/// One normalized Review finding with optional structured source evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFinding {
    /// Review authority classification used by routing.
    pub class: ReviewFindingClass,
    /// Concise finding title.
    pub title: String,
    /// Human-readable explanation and impact.
    pub body: String,
    /// Backend-supplied severity; routing remains based on `class`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    /// Repository-relative file containing the evidence, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// One-based source line associated with `file`, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Concrete command, diff, or code evidence supporting the finding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

/// Backend-neutral terminal Review report consumed by existing routing logic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentReviewReport {
    /// Backend that produced the report.
    pub reviewer_backend: String,
    /// Normalized findings from the backend response.
    #[serde(default)]
    pub findings: Vec<ReviewFinding>,
    /// Concise review summary.
    pub summary: Option<String>,
    /// Raw or reconstructed backend result text.
    pub stdout: Option<String>,
    /// Backend stderr captured for diagnostics.
    pub stderr: Option<String>,
    /// Process exit status captured by the backend.
    pub exit_status: Option<String>,
    /// Provider session or thread identity used by this review.
    pub session_id: Option<String>,
}

impl AgentReviewReport {
    pub(crate) fn has_parsed_review_result(&self) -> bool {
        [
            self.summary.as_deref(),
            self.stdout.as_deref(),
            self.stderr.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|text| parse_review_result(text).is_some())
    }

    /// Returns whether a confirmed non-UAT finding requires implementation rework.
    pub fn blocks_progress(&self) -> bool {
        self.findings.iter().any(review_finding_blocks_progress)
    }

    /// Returns whether missing context prevents a conclusive review pass.
    pub fn is_inconclusive(&self) -> bool {
        self.inconclusive_reason().is_some()
    }

    /// Explains why this report cannot support a pass, when applicable.
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

    /// Compatibility alias for callers that gate Human Review directly.
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

/// Parses the legacy bracketed text protocol used by Gemini and agy backends.
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
        severity: None,
        file: None,
        line: None,
        evidence: None,
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
        severity: None,
        file: None,
        line: None,
        evidence: None,
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
        severity: None,
        file: None,
        line: None,
        evidence: None,
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
