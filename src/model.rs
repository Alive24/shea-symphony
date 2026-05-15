use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
