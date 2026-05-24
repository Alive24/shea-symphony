use serde::{Deserialize, Serialize};

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
