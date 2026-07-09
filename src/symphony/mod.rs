//! Temporal-backed orchestration boundary for Shea Symphony 2607.
//!
//! A Symphony Workflow owns replay-deterministic, per-issue orchestration.
//! Network, tracker, filesystem, agent, and SQLite access must cross an
//! Activity boundary so replay never repeats an uncontrolled side effect.
//! Task queues isolate control-plane, long-running agent, and local projection
//! capacity. The local-state schema in this module is a rebuildable read-model
//! contract only; neither it nor the App may authorize workflow progression or
//! replace tracker truth.
//!
//! Public Workflow and Activity type names and serialized DTO fields are
//! durable compatibility surfaces once recorded in Temporal history. Change
//! them only with an explicit compatibility and replay plan.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

mod activities;
mod client;
mod dto;
mod local_state;
mod task_queues;
mod worker_runtime;
mod workers;
mod workflows;

pub use client::{StartedIssueWorkflow, SymphonyTemporalClient, TemporalRuntimeError};
pub use dto::{IssueWorkflowInput, IssueWorkflowQueryResult, IssueWorkflowState};
pub use local_state::{
    ActiveWorkflowGuardDescriptor, ColumnDescriptor, ColumnKind, Freshness, IndexDescriptor,
    IssueRef, LocalStateSchema, PrimaryKeyDescriptor, RepoId, TableDescriptor, TrackerBackend,
    WorkflowId, WorkflowIndexStatus, ACTIVE_WORKFLOW_STATUSES, LOCAL_STATE_SCHEMA,
};
pub use task_queues::{AGENT_TASK_QUEUE, CORE_TASK_QUEUE, LOCAL_TASK_QUEUE};
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
