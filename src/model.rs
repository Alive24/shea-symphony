use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockerRef {
    pub id: Option<String>,
    pub identifier: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedPullRequest {
    pub id: Option<String>,
    pub number: Option<u64>,
    pub url: Option<String>,
    pub state: Option<String>,
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
pub struct RetrySnapshot {
    pub issue_id: String,
    pub identifier: String,
    pub attempt: u32,
    pub due_in_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TokenTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub seconds_running: u64,
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
    pub skipped: Vec<SkippedIssue>,
    #[serde(default)]
    pub integration_gaps: Vec<String>,
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
