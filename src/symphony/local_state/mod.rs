//! Typed lifecycle for Symphony's rebuildable machine-local SQLite read model.
//!
//! The database accelerates local reads and records projection state. Temporal
//! history and the configured tracker remain authoritative for workflow and
//! tracker decisions.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

pub(crate) mod admin;
mod database;
mod identity;
mod migration;
pub(crate) mod projector;

pub use database::{JournalMode, LocalStateDatabase, LocalStateError, LocalStateInitialization};
pub use identity::{
    Freshness, IssueRef, RepoId, TrackerBackend, WorkflowId, WorkflowIndexStatus,
    WorkspaceRuntimeId, ACTIVE_WORKFLOW_STATUSES,
};
