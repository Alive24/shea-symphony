pub mod activities;
pub mod client;
pub mod dto;
pub mod local_state;
pub mod task_queues;
pub mod worker_runtime;
pub mod workers;
pub mod workflows;

pub use client::{StartedIssueWorkflow, SymphonyTemporalClient, TemporalRuntimeError};
pub use dto::{
    IssueWorkflowInput, IssueWorkflowQueryResult, IssueWorkflowState, NoopActivityRequest,
    NoopActivityResult,
};
pub use local_state::{
    ActiveWorkflowGuardDescriptor, ColumnDescriptor, ColumnKind, Freshness, IndexDescriptor,
    IssueRef, LocalStateSchema, PrimaryKeyDescriptor, RepoId, TableDescriptor, TrackerBackend,
    WorkflowId, WorkflowIndexStatus, ACTIVE_WORKFLOW_STATUSES, LOCAL_STATE_SCHEMA,
};
pub use task_queues::{AGENT_TASK_QUEUE, CORE_TASK_QUEUE, LOCAL_TASK_QUEUE, TASK_QUEUE_COUNT};
pub use worker_runtime::run_symphony_workers;
pub use workers::{task_queue_registrations, TaskQueueRegistration};

#[cfg(test)]
mod tests {
    use super::{WorkflowIndexStatus, LOCAL_STATE_SCHEMA};

    #[test]
    fn local_state_contract_is_exported_from_symphony_boundary() {
        assert!(WorkflowIndexStatus::Starting.is_active());
        assert!(LOCAL_STATE_SCHEMA.table("workflow_index").is_some());
    }
}
