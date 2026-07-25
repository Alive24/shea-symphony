//! Shared typed representation of the v1 `workflow_index` storage contract.
//!
//! Projection and query code use the same row decoder so an active read cannot
//! reinterpret lifecycle, freshness, or terminal values.

use std::io;

use rusqlite::types::Type;

use super::{Freshness, WorkflowIndexStatus};

/// A bounded terminal classification persisted in `workflow_index.terminal_outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkflowTerminalOutcome {
    /// The execution completed successfully.
    Completed,
    /// The execution failed.
    Failed,
    /// The execution was cancelled before normal completion.
    Cancelled,
    /// The execution was terminated by an operator or policy.
    Terminated,
    /// The execution timed out.
    TimedOut,
    /// A Describe-backed close was observed without a supported classification.
    ClosedUnknown,
}

impl WorkflowTerminalOutcome {
    /// Returns the stable v1 storage spelling of this outcome.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Terminated => "terminated",
            Self::TimedOut => "timed_out",
            Self::ClosedUnknown => "closed_unknown",
        }
    }

    fn from_storage(value: &str) -> Option<Self> {
        match value {
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "terminated" => Some(Self::Terminated),
            "timed_out" => Some(Self::TimedOut),
            "closed_unknown" => Some(Self::ClosedUnknown),
            _ => None,
        }
    }
}

/// A row read from the committed v1 `workflow_index` schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowIndexRow {
    /// Persisted Symphony semantic Workflow ID.
    pub(crate) workflow_id: String,
    /// Persisted Temporal Run ID when Describe evidence has supplied one.
    pub(crate) run_id: Option<String>,
    /// Persisted stable runtime scope.
    pub(crate) workspace_runtime_id: String,
    /// Persisted repository storage key.
    pub(crate) repo_id: String,
    /// Persisted tracker issue storage key.
    pub(crate) issue_ref: String,
    /// Persisted activation tracker state.
    pub(crate) from_state: String,
    /// Persisted requested orchestration target.
    pub(crate) target_kind: String,
    /// Persisted caller-supplied tracker context.
    pub(crate) current_state: String,
    /// Persisted bounded lifecycle sentinel.
    pub(crate) active_step: String,
    /// Persisted waiting sentinel, unused by this summary slice.
    pub(crate) waiting_kind: Option<String>,
    /// Persisted activation provenance reference.
    pub(crate) source_ref: String,
    /// Persisted activation tracker revision.
    pub(crate) source_tracker_revision: String,
    /// Persisted authoritative Temporal execution start time.
    pub(crate) started_at: String,
    /// Persisted progress time, unchanged by this summary slice.
    pub(crate) last_progress_at: Option<String>,
    /// Persisted local lifecycle classification.
    pub(crate) status: WorkflowIndexStatus,
    /// Persisted bounded close classification.
    pub(crate) terminal_outcome: Option<WorkflowTerminalOutcome>,
    /// Persisted optional operator-action provenance.
    pub(crate) operator_action_ref: Option<String>,
    /// Persisted material-projection freshness.
    pub(crate) freshness: Freshness,
    /// Persisted observation time for the last material projection.
    pub(crate) updated_at: String,
}

pub(super) fn workflow_index_columns() -> [&'static str; 19] {
    [
        "workflow_id",
        "run_id",
        "workspace_runtime_id",
        "repo_id",
        "issue_ref",
        "from_state",
        "target_kind",
        "current_state",
        "active_step",
        "waiting_kind",
        "source_ref",
        "source_tracker_revision",
        "started_at",
        "last_progress_at",
        "status",
        "terminal_outcome",
        "operator_action_ref",
        "freshness",
        "updated_at",
    ]
}

pub(super) fn workflow_index_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowIndexRow> {
    let status_value: String = row.get(14)?;
    let terminal_outcome_value: Option<String> = row.get(15)?;
    let freshness_value: String = row.get(17)?;

    Ok(WorkflowIndexRow {
        workflow_id: row.get(0)?,
        run_id: row.get(1)?,
        workspace_runtime_id: row.get(2)?,
        repo_id: row.get(3)?,
        issue_ref: row.get(4)?,
        from_state: row.get(5)?,
        target_kind: row.get(6)?,
        current_state: row.get(7)?,
        active_step: row.get(8)?,
        waiting_kind: row.get(9)?,
        source_ref: row.get(10)?,
        source_tracker_revision: row.get(11)?,
        started_at: row.get(12)?,
        last_progress_at: row.get(13)?,
        status: workflow_index_status_from_storage(&status_value)
            .ok_or_else(|| unsupported_storage_value(14, "status", &status_value))?,
        terminal_outcome: terminal_outcome_value
            .as_deref()
            .map(|value| {
                WorkflowTerminalOutcome::from_storage(value)
                    .ok_or_else(|| unsupported_storage_value(15, "terminal_outcome", value))
            })
            .transpose()?,
        operator_action_ref: row.get(16)?,
        freshness: freshness_from_storage(&freshness_value)
            .ok_or_else(|| unsupported_storage_value(17, "freshness", &freshness_value))?,
        updated_at: row.get(18)?,
    })
}

fn workflow_index_status_from_storage(value: &str) -> Option<WorkflowIndexStatus> {
    match value {
        "starting" => Some(WorkflowIndexStatus::Starting),
        "running" => Some(WorkflowIndexStatus::Running),
        "completed" => Some(WorkflowIndexStatus::Completed),
        "failed" => Some(WorkflowIndexStatus::Failed),
        "start_failed" => Some(WorkflowIndexStatus::StartFailed),
        "stale_start" => Some(WorkflowIndexStatus::StaleStart),
        "stale_missing" => Some(WorkflowIndexStatus::StaleMissing),
        "closed_unknown" => Some(WorkflowIndexStatus::ClosedUnknown),
        _ => None,
    }
}

fn freshness_from_storage(value: &str) -> Option<Freshness> {
    match value {
        "fresh" => Some(Freshness::Fresh),
        "stale" => Some(Freshness::Stale),
        "refreshing" => Some(Freshness::Refreshing),
        "failed" => Some(Freshness::Failed),
        "unknown" => Some(Freshness::Unknown),
        _ => None,
    }
}

pub(super) fn unsupported_storage_value(
    index: usize,
    column: &str,
    value: &str,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Text,
        Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported workflow_index {column} value {value:?}"),
        )),
    )
}
