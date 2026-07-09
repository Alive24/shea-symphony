use std::time::Duration;

use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{ActivityOptions, WorkflowContext, WorkflowContextView, WorkflowResult};

use crate::symphony::activities::CoreActivities;
use crate::symphony::dto::{
    IssueWorkflowInput, IssueWorkflowQueryResult, IssueWorkflowState, NoopActivityRequest,
};

pub const ISSUE_WORKFLOW_TYPE: &str = "IssueWorkflow";
pub const ISSUE_WORKFLOW_QUERY_CURRENT_STATE: &str = "current_state";

#[workflow(name = "IssueWorkflow")]
pub struct IssueWorkflow {
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
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        _input: IssueWorkflowInput,
    ) -> WorkflowResult<IssueWorkflowState> {
        let request = ctx.state(|state| NoopActivityRequest {
            workflow_id: state.workflow_id.clone(),
            activity_kind: "NoopCoreActivity".to_string(),
            issue_ref: state.issue_ref.clone(),
        });

        let _noop = ctx
            .start_activity(
                CoreActivities::noop_core_activity,
                request,
                ActivityOptions::start_to_close_timeout(Duration::from_secs(30)),
            )
            .await?;

        ctx.state_mut(|state| {
            state.active_step = "noop_completed".to_string();
            state.terminal_outcome = Some("completed_noop".to_string());
            state.runtime_health_summary = "completed no-op IssueWorkflow path".to_string();
        });

        Ok(ctx.state(|state| state.clone()))
    }

    #[query(name = "current_state")]
    pub fn current_state(&self, _ctx: &WorkflowContextView) -> IssueWorkflowQueryResult {
        self.state.query_result()
    }
}
