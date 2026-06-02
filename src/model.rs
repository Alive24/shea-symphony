use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockerRef {
    pub id: Option<String>,
    pub identifier: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedPullRequest {
    pub id: Option<String>,
    pub number: Option<u64>,
    pub url: Option<String>,
    pub state: Option<String>,
    #[serde(default)]
    pub is_draft: Option<bool>,
    #[serde(default)]
    pub merge_state_status: Option<String>,
    #[serde(default)]
    pub review_decision: Option<String>,
    #[serde(default)]
    pub base_ref_name: Option<String>,
    #[serde(default)]
    pub head_ref_name: Option<String>,
    #[serde(default)]
    pub source: LinkedPullRequestSource,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkedPullRequestSource {
    #[default]
    Unknown,
    GithubNative,
    FallbackDiagnostic,
}

impl LinkedPullRequest {
    pub fn is_github_native_linkage(&self) -> bool {
        self.source == LinkedPullRequestSource::GithubNative
            || (self.source == LinkedPullRequestSource::Unknown
                && self.id.as_deref().is_some_and(|id| !id.is_empty()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackerIssue {
    pub tracker_kind: String,
    pub id: String,
    pub item_id: Option<String>,
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    pub url: Option<String>,
    pub state: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub assignees: Vec<String>,
    pub priority: Option<i64>,
    pub branch_name: Option<String>,
    #[serde(default)]
    pub linked_pull_requests: Vec<LinkedPullRequest>,
    #[serde(default)]
    pub blocked_by: Vec<BlockerRef>,
    #[serde(default)]
    pub project_fields: BTreeMap<String, serde_json::Value>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSubissueStatus {
    pub identifier: String,
    pub project_state: Option<String>,
    pub github_state: Option<String>,
}

impl TrackerIssue {
    pub fn normalized_state(&self) -> String {
        normalize_state(&self.state)
    }

    pub fn labels_lowercase(&self) -> Vec<String> {
        self.labels
            .iter()
            .map(|label| label.to_lowercase())
            .collect()
    }
}

pub fn native_parent_identifier(issue: &TrackerIssue) -> Option<String> {
    issue
        .project_fields
        .get("GitHub Native Parent")
        .and_then(|value| {
            value
                .get("identifier")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .or_else(|| issue_ref_from_value(value))
        })
        .or_else(|| {
            issue
                .project_fields
                .get("Native Parent Issue")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
}

pub fn is_native_subissue(issue: &TrackerIssue) -> bool {
    native_parent_identifier(issue).is_some()
}

pub fn native_subissue_human_review_exception(issue: &TrackerIssue) -> bool {
    let exception_fields = [
        "Subissue Human Review Exception",
        "Direct Human Review Required",
        "subissue_human_review_exception",
        "direct_human_review_required",
    ];
    if exception_fields
        .iter()
        .any(|field| explicit_truthy_project_field(issue, field))
    {
        return true;
    }

    issue
        .description
        .as_deref()
        .map(|description| {
            let text = description.to_ascii_lowercase();
            text.contains("subissue human review exception:")
                || text.contains("direct human review exception:")
                || text.contains("direct human review required: yes")
                || text.contains("requires direct human review: yes")
                || text.contains("requires routine direct human review: yes")
        })
        .unwrap_or(false)
}

pub fn native_subissue_statuses(issue: &TrackerIssue) -> Vec<NativeSubissueStatus> {
    let mut statuses = Vec::new();
    if let Some(values) = issue
        .project_fields
        .get("GitHub Native Subissues")
        .and_then(serde_json::Value::as_array)
    {
        for value in values {
            if let Some(identifier) = issue_ref_from_value(value) {
                push_native_subissue_status(
                    &mut statuses,
                    NativeSubissueStatus {
                        identifier,
                        project_state: value
                            .get("project_state")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                        github_state: value
                            .get("state")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                    },
                );
            }
        }
    }

    if let Some(project_states) = issue
        .project_fields
        .get("Native Subissue Project States")
        .and_then(serde_json::Value::as_str)
    {
        for (identifier, state) in parse_issue_state_pairs(project_states) {
            push_native_subissue_status(
                &mut statuses,
                NativeSubissueStatus {
                    identifier,
                    project_state: Some(state),
                    github_state: None,
                },
            );
        }
    }

    if let Some(subissues) = issue
        .project_fields
        .get("Native Subissues")
        .and_then(serde_json::Value::as_str)
    {
        for identifier in subissues
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            push_native_subissue_status(
                &mut statuses,
                NativeSubissueStatus {
                    identifier: identifier.to_string(),
                    project_state: None,
                    github_state: None,
                },
            );
        }
    }

    statuses
}

pub fn incomplete_native_subissues(
    issue: &TrackerIssue,
    terminal_states: &BTreeSet<String>,
) -> Vec<NativeSubissueStatus> {
    native_subissue_statuses(issue)
        .into_iter()
        .filter(|subissue| {
            subissue
                .project_state
                .as_deref()
                .map(normalize_state)
                .map(|state| !terminal_states.contains(&state))
                .unwrap_or(true)
        })
        .collect()
}

pub fn native_subissue_gate_blocker(
    issue: &TrackerIssue,
    terminal_states: &BTreeSet<String>,
) -> Option<String> {
    let incomplete = incomplete_native_subissues(issue, terminal_states);
    if incomplete.is_empty() {
        return None;
    }

    Some(format!(
        "blocked by incomplete native subissues: {}",
        incomplete
            .iter()
            .map(|subissue| {
                let state = subissue
                    .project_state
                    .as_deref()
                    .unwrap_or("missing Project status");
                format!("{}={state}", subissue.identifier)
            })
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn explicit_truthy_project_field(issue: &TrackerIssue, field: &str) -> bool {
    issue
        .project_fields
        .get(field)
        .is_some_and(|value| match value {
            serde_json::Value::Bool(value) => *value,
            serde_json::Value::String(value) => {
                let normalized = value.trim().to_ascii_lowercase();
                !normalized.is_empty()
                    && !matches!(
                        normalized.as_str(),
                        "false" | "no" | "none" | "n/a" | "not required"
                    )
            }
            _ => false,
        })
}

fn push_native_subissue_status(
    statuses: &mut Vec<NativeSubissueStatus>,
    mut candidate: NativeSubissueStatus,
) {
    if let Some(existing) = statuses
        .iter_mut()
        .find(|status| issue_refs_match(&status.identifier, &candidate.identifier))
    {
        if existing.project_state.is_none() {
            existing.project_state = candidate.project_state.take();
        }
        if existing.github_state.is_none() {
            existing.github_state = candidate.github_state.take();
        }
        return;
    }
    statuses.push(candidate);
}

fn issue_ref_from_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("identifier")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| value.as_str().map(str::to_string))
        .or_else(|| {
            value
                .get("number")
                .and_then(serde_json::Value::as_u64)
                .map(|number| format!("#{number}"))
        })
}

fn parse_issue_state_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split(',')
        .filter_map(|pair| {
            let (issue_ref, state) = pair.trim().split_once('=')?;
            let issue_ref = issue_ref.trim();
            let state = state.trim();
            if issue_ref.is_empty() || state.is_empty() {
                None
            } else {
                Some((issue_ref.to_string(), state.to_string()))
            }
        })
        .collect()
}

fn issue_refs_match(left: &str, right: &str) -> bool {
    normalize_issue_ref(left) == normalize_issue_ref(right)
}

fn normalize_issue_ref(value: &str) -> String {
    value.trim().trim_start_matches('#').to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GateDecisionKind {
    Ready,
    ReadyWithAssumptions,
    NeedToClarify,
    TooBroad,
    Blocked,
    DuplicateAlreadyCovered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateDecision {
    pub kind: GateDecisionKind,
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl GateDecision {
    pub fn ready() -> Self {
        Self {
            kind: GateDecisionKind::Ready,
            missing: Vec::new(),
            assumptions: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn is_dispatchable(&self) -> bool {
        matches!(
            self.kind,
            GateDecisionKind::Ready | GateDecisionKind::ReadyWithAssumptions
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningSnapshot {
    pub issue_id: String,
    pub identifier: String,
    pub state: String,
    pub backend: String,
    pub workspace_path: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub instance_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStatusSnapshot {
    pub session_id: String,
    pub lane: String,
    #[serde(default)]
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub status: String,
    pub evidence_source: String,
    pub evidence: String,
    #[serde(default)]
    pub issue_identifier: Option<String>,
    #[serde(default)]
    pub issue_title: Option<String>,
    #[serde(default)]
    pub attach_command: Option<String>,
    #[serde(default)]
    pub log_path: Option<String>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrySnapshot {
    pub issue_id: String,
    pub identifier: String,
    pub attempt: u32,
    pub due_in_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestStatus {
    pub lane: String,
    pub category: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TokenTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub seconds_running: u64,
}

impl TokenTotals {
    pub fn from_agent_events(events: &[AgentEvent]) -> Self {
        let mut totals = Self::default();
        let mut latest_reported_total = 0;

        for event in events {
            if let AgentEvent::TokenUsage {
                input_tokens,
                output_tokens,
                total_tokens,
                ..
            } = event
            {
                totals.input_tokens = totals.input_tokens.saturating_add(*input_tokens);
                totals.output_tokens = totals.output_tokens.saturating_add(*output_tokens);
                latest_reported_total = *total_tokens;
            }
        }

        let summed_total = totals.input_tokens.saturating_add(totals.output_tokens);
        totals.total_tokens = summed_total.max(latest_reported_total);
        totals
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RuntimeSnapshot {
    #[serde(default)]
    pub planned: Vec<RunningSnapshot>,
    #[serde(default)]
    pub running: Vec<RunningSnapshot>,
    #[serde(default)]
    pub retrying: Vec<RetrySnapshot>,
    pub codex_totals: TokenTotals,
    pub polling: PollingSnapshot,
    #[serde(default)]
    pub sessions: Vec<SessionStatusSnapshot>,
    #[serde(default)]
    pub skipped: Vec<SkippedIssue>,
    #[serde(default)]
    pub integration_gaps: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_status: Option<LatestStatus>,
    pub event_log_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingSnapshot {
    pub checking: bool,
    pub next_poll_in_ms: Option<u64>,
    pub poll_interval_ms: u64,
}

impl Default for PollingSnapshot {
    fn default() -> Self {
        Self {
            checking: false,
            next_poll_in_ms: None,
            poll_interval_ms: 30_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedIssue {
    pub issue_id: String,
    pub identifier: String,
    pub reason: String,
    pub gate: Option<GateDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentEvent {
    SessionStarted {
        backend: String,
        session_id: String,
    },
    Message {
        backend: String,
        session_id: Option<String>,
        text: String,
    },
    TokenUsage {
        backend: String,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    },
    Completed {
        backend: String,
        session_id: Option<String>,
        summary: String,
    },
    Failed {
        backend: String,
        error: String,
    },
}

pub fn normalize_state(state: &str) -> String {
    state.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_totals_default_when_events_have_no_usage() {
        let totals = TokenTotals::from_agent_events(&[
            AgentEvent::SessionStarted {
                backend: "dry-run".into(),
                session_id: "session-1".into(),
            },
            AgentEvent::Completed {
                backend: "dry-run".into(),
                session_id: Some("session-1".into()),
                summary: "done".into(),
            },
        ]);

        assert_eq!(totals, TokenTotals::default());
    }

    #[test]
    fn token_totals_accumulate_input_and_output_usage() {
        let totals = TokenTotals::from_agent_events(&[
            AgentEvent::TokenUsage {
                backend: "codex".into(),
                input_tokens: 10,
                output_tokens: 4,
                total_tokens: 14,
            },
            AgentEvent::TokenUsage {
                backend: "codex".into(),
                input_tokens: 3,
                output_tokens: 8,
                total_tokens: 11,
            },
        ]);

        assert_eq!(totals.input_tokens, 13);
        assert_eq!(totals.output_tokens, 12);
        assert_eq!(totals.total_tokens, 25);
    }

    #[test]
    fn token_totals_preserve_latest_absolute_total_when_larger() {
        let totals = TokenTotals::from_agent_events(&[
            AgentEvent::TokenUsage {
                backend: "codex".into(),
                input_tokens: 10,
                output_tokens: 4,
                total_tokens: 14,
            },
            AgentEvent::TokenUsage {
                backend: "codex".into(),
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 40,
            },
        ]);

        assert_eq!(totals.input_tokens, 11);
        assert_eq!(totals.output_tokens, 5);
        assert_eq!(totals.total_tokens, 40);
    }
}
