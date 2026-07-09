//! Serialized contracts exchanged with Temporal Workflows and Activities.
//!
//! Workflow inputs, state, Activity payloads, and terminal results can be
//! persisted in Temporal history and replayed long after the originating process
//! exits. Keep these DTOs backward-compatible and intentionally small. Large
//! transcripts, diffs, tracker payloads, and report bodies belong in referenced
//! artifacts or rebuildable local projections, not in history.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Durable input used to start one executable Issue Workflow episode.
///
/// Every field may be recorded in Temporal history. Add fields only when replay
/// and orchestration require the value; prefer stable references over embedded
/// issue bodies, comments, logs, transcripts, or diffs.
pub struct IssueWorkflowInput {
    /// Stable, application-assigned Temporal Workflow ID for this execution.
    pub workflow_id: String,
    /// Stable repository identity, normally host/owner/repository derived.
    pub repo_id: String,
    /// Tracker-native issue reference such as `#477`.
    pub issue_ref: String,
    /// Authoritative tracker state that made this execution eligible to start.
    pub from_tracker_state: String,
    /// Requested orchestration target, such as contract check or agent work.
    pub target_kind: String,
    /// Reference to the tracker transition or operator action that activated work.
    pub source_ref: String,
    /// Tracker revision observed when the start decision was made.
    pub source_tracker_revision: String,
    /// Human-readable UTC timestamp captured outside deterministic Workflow code.
    pub started_at: String,
    /// Optional durable reference to the operator action that requested execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_action_ref: Option<String>,
    /// Optional reference to the capacity policy used when admitting the work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_policy_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Replay-derived state owned by an Issue Workflow execution.
///
/// This is authoritative for the execution's orchestration decisions, but not
/// for external tracker state. Artifact bodies remain outside this DTO and are
/// represented only by stable references.
pub struct IssueWorkflowState {
    /// Stable application-assigned Workflow ID.
    pub workflow_id: String,
    /// Temporal Run ID when known to the Workflow state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Stable repository identity associated with the issue.
    pub repo_id: String,
    /// Tracker-native issue reference.
    pub issue_ref: String,
    /// Last tracker state accepted by the Workflow's ordered decision logic.
    pub current_tracker_state: String,
    /// Current deterministic orchestration step.
    pub active_step: String,
    /// Terminal outcome once the execution has completed or failed permanently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_outcome: Option<String>,
    /// Stable references to external artifacts; never embedded artifact bodies.
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    /// Compact operator-facing health summary derived by the Workflow.
    pub runtime_health_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Read-only projection returned by the Issue Workflow state Query.
///
/// A query result is an observation of replay-derived Workflow state. It cannot
/// authorize tracker writes or imply that an external side effect succeeded.
pub struct IssueWorkflowQueryResult {
    /// Stable application-assigned Workflow ID.
    pub workflow_id: String,
    /// Temporal Run ID when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Tracker-native issue reference.
    pub issue_ref: String,
    /// Last tracker state accepted by Workflow decision logic.
    pub current_tracker_state: String,
    /// Current deterministic orchestration step.
    pub active_step: String,
    /// Terminal outcome when the execution has reached one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_outcome: Option<String>,
    /// Compact operator-facing runtime health summary.
    pub runtime_health_summary: String,
    /// Stable references to external artifacts relevant to the current state.
    #[serde(default)]
    pub artifact_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Internal payload used to prove durable Activity scheduling and routing.
pub struct NoopActivityRequest {
    /// Workflow that scheduled the Activity.
    pub(crate) workflow_id: String,
    /// Durable Activity type name being exercised.
    pub(crate) activity_kind: String,
    /// Tracker issue associated with the Activity.
    pub(crate) issue_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Internal compact result returned by placeholder Activities.
pub struct NoopActivityResult {
    /// Machine-readable placeholder outcome.
    pub(crate) outcome: String,
    /// Short operator-readable description; never a transcript or report body.
    pub(crate) summary: String,
    /// Stable references to external artifacts produced by an Activity.
    #[serde(default)]
    pub(crate) artifact_refs: Vec<String>,
}

impl IssueWorkflowState {
    /// Initializes deterministic Workflow state from a durable start payload.
    pub(crate) fn from_input(input: IssueWorkflowInput, run_id: Option<String>) -> Self {
        Self {
            workflow_id: input.workflow_id,
            run_id,
            repo_id: input.repo_id,
            issue_ref: input.issue_ref,
            current_tracker_state: input.from_tracker_state,
            active_step: format!("noop:{}", input.target_kind),
            terminal_outcome: None,
            artifact_refs: Vec::new(),
            runtime_health_summary: "initialized".to_string(),
        }
    }

    /// Builds the bounded, read-only Query projection of the current state.
    pub(crate) fn query_result(&self) -> IssueWorkflowQueryResult {
        IssueWorkflowQueryResult {
            workflow_id: self.workflow_id.clone(),
            run_id: self.run_id.clone(),
            issue_ref: self.issue_ref.clone(),
            current_tracker_state: self.current_tracker_state.clone(),
            active_step: self.active_step.clone(),
            terminal_outcome: self.terminal_outcome.clone(),
            runtime_health_summary: self.runtime_health_summary.clone(),
            artifact_refs: self.artifact_refs.clone(),
        }
    }
}

impl NoopActivityResult {
    /// Describes a successful no-op without claiming an external write occurred.
    pub(crate) fn success(request: &NoopActivityRequest) -> Self {
        Self {
            outcome: "noop_success".to_string(),
            summary: format!(
                "{} completed without side effects for {}",
                request.activity_kind, request.issue_ref
            ),
            artifact_refs: Vec::new(),
        }
    }

    /// Describes an intentionally inert placeholder Activity.
    pub(crate) fn not_implemented(request: &NoopActivityRequest) -> Self {
        Self {
            outcome: "not_implemented".to_string(),
            summary: format!(
                "{} is registered but intentionally inert for {}",
                request.activity_kind, request.issue_ref
            ),
            artifact_refs: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> IssueWorkflowInput {
        IssueWorkflowInput {
            workflow_id: "issue:shea-symphony:475:pulse:todo-to-work:20260709-175700Z:project"
                .to_string(),
            repo_id: "Alive24/shea-symphony".to_string(),
            issue_ref: "#475".to_string(),
            from_tracker_state: "Todo".to_string(),
            target_kind: "work".to_string(),
            source_ref: "project-v2".to_string(),
            source_tracker_revision: "rev-1".to_string(),
            started_at: "2026-07-09T17:57:00Z".to_string(),
            operator_action_ref: None,
            capacity_policy_ref: Some("default-local".to_string()),
        }
    }

    #[test]
    fn issue_workflow_state_is_small_serializable_dto() {
        let state = IssueWorkflowState::from_input(input(), Some("temporal-run-id".to_string()));
        let value = serde_json::to_value(&state).unwrap();

        assert_eq!(value["issue_ref"], "#475");
        assert_eq!(value["artifact_refs"].as_array().unwrap().len(), 0);
        assert!(value.get("transcript").is_none());
        assert!(value.get("diff").is_none());
        assert!(value.get("tracker_payload").is_none());
    }

    #[test]
    fn query_result_exposes_state_without_repo_payload() {
        let state = IssueWorkflowState::from_input(input(), None);
        let query = state.query_result();

        assert_eq!(query.workflow_id, state.workflow_id);
        assert_eq!(query.issue_ref, "#475");
        assert!(!serde_json::to_value(query)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("repo_id"));
    }

    #[test]
    fn noop_activity_results_carry_summaries_not_artifacts() {
        let request = NoopActivityRequest {
            workflow_id: "workflow-id".to_string(),
            activity_kind: "NoopCoreActivity".to_string(),
            issue_ref: "#475".to_string(),
        };

        let result = NoopActivityResult::success(&request);

        assert_eq!(result.outcome, "noop_success");
        assert!(result.summary.contains("NoopCoreActivity"));
        assert!(result.artifact_refs.is_empty());
    }
}
