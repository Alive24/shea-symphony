//! Replay-deterministic Temporal Workflow definitions for Symphony.
//!
//! Workflow code may inspect and mutate only durable Workflow state, schedule
//! Activities, and process Temporal messages. It must not read the wall clock,
//! filesystem, network, tracker, SQLite database, or agent process directly.
//! Those operations belong behind Activities so Temporal replay cannot repeat an
//! uncontrolled side effect.

use std::time::Duration;

use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{ActivityOptions, WorkflowContext, WorkflowContextView, WorkflowResult};

use crate::symphony::activities::CoreActivities;
use crate::symphony::dto::{
    IssueWorkflowInput, IssueWorkflowQueryResult, IssueWorkflowState, NoopActivityRequest,
};

/// Durable Temporal Workflow type name for executable issue episodes.
///
/// Existing histories refer to this exact value. A rename requires a worker
/// compatibility plan for in-flight and replayed executions.
pub(super) const ISSUE_WORKFLOW_TYPE: &str = "IssueWorkflow";

#[workflow(name = "IssueWorkflow")]
/// Deterministic state machine for one executable issue orchestration episode.
pub(super) struct IssueWorkflow {
    state: IssueWorkflowState,
}

#[workflow_methods]
impl IssueWorkflow {
    #[init]
    fn new(_ctx: &WorkflowContextView, input: IssueWorkflowInput) -> Self {
        Self {
            state: IssueWorkflowState::from_input(input, None),
        }
    }

    #[run]
    /// Runs the quickstart execution path and returns its terminal durable state.
    ///
    /// The method schedules external work through Activities only. Returning
    /// `Ok` means the Workflow path completed; the Activity result must still be
    /// interpreted according to its own outcome contract.
    pub async fn run(ctx: &mut WorkflowContext<Self>) -> WorkflowResult<IssueWorkflowState> {
        // The pinned SDK delivers a start input to either `#[init]` or `#[run]`.
        // `#[init]` owns it here, so this method reads only durable state rather
        // than requesting the already-consumed input during a Workflow Task.
        // Workflow code must stay replay-deterministic. Even this skeleton only
        // schedules Activity work; filesystem, network, tracker, and clock I/O
        // belong behind Activity boundaries.
        let request = ctx.state(|workflow| NoopActivityRequest {
            workflow_id: workflow.state.workflow_id.clone(),
            activity_kind: "NoopCoreActivity".to_string(),
            issue_ref: workflow.state.issue_ref.clone(),
        });

        let noop = ctx
            .start_activity(
                CoreActivities::noop_core_activity,
                request,
                ActivityOptions::start_to_close_timeout(Duration::from_secs(30)),
            )
            .await?;

        ctx.state_mut(|workflow| {
            workflow.state.artifact_refs = noop.artifact_refs;
            workflow.state.runtime_health_summary = noop.summary;
            if noop.outcome == "noop_success" {
                workflow.state.active_step = "noop_completed".to_string();
                workflow.state.terminal_outcome = Some("completed_noop".to_string());
            } else {
                // Placeholder Activity results are part of the durable no-op
                // contract. Preserve a non-success outcome instead of claiming
                // the core no-op path completed when it did not.
                workflow.state.active_step = "noop_activity_incomplete".to_string();
                workflow.state.terminal_outcome = Some(noop.outcome);
            }
        });

        Ok(ctx.state(|workflow| workflow.state.clone()))
    }

    #[query(name = "current_state")]
    /// Returns a bounded, read-only projection of replay-derived Workflow state.
    ///
    /// Query handlers cannot perform Activities or external I/O and therefore do
    /// not authorize tracker progression.
    pub fn current_state(&self, _ctx: &WorkflowContextView) -> IssueWorkflowQueryResult {
        self.state.query_result()
    }
}

#[cfg(test)]
mod tests {
    use super::ISSUE_WORKFLOW_TYPE;

    #[test]
    fn durable_workflow_names_match_registered_contract() {
        assert_eq!(ISSUE_WORKFLOW_TYPE, "IssueWorkflow");
    }
}
