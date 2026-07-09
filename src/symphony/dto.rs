use serde::{Deserialize, Serialize};

// Workflow DTOs are intentionally small because every field can become durable
// Temporal history. Large details belong in artifacts, tracker comments, or
// the local SQLite read model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueWorkflowInput {
    pub workflow_id: String,
    pub repo_id: String,
    pub issue_ref: String,
    pub from_tracker_state: String,
    pub target_kind: String,
    pub source_ref: String,
    pub source_tracker_revision: String,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_action_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_policy_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueWorkflowState {
    pub workflow_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub repo_id: String,
    pub issue_ref: String,
    pub current_tracker_state: String,
    pub active_step: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_outcome: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    pub runtime_health_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueWorkflowQueryResult {
    pub workflow_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub issue_ref: String,
    pub current_tracker_state: String,
    pub active_step: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_outcome: Option<String>,
    pub runtime_health_summary: String,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoopActivityRequest {
    pub workflow_id: String,
    pub activity_kind: String,
    pub issue_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoopActivityResult {
    pub outcome: String,
    pub summary: String,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
}

impl IssueWorkflowState {
    pub fn from_input(input: IssueWorkflowInput, run_id: Option<String>) -> Self {
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

    pub fn query_result(&self) -> IssueWorkflowQueryResult {
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
    pub fn success(request: &NoopActivityRequest) -> Self {
        Self {
            outcome: "noop_success".to_string(),
            summary: format!(
                "{} completed without side effects for {}",
                request.activity_kind, request.issue_ref
            ),
            artifact_refs: Vec::new(),
        }
    }

    pub fn not_implemented(request: &NoopActivityRequest) -> Self {
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
