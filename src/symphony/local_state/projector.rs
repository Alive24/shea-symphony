//! Describe-backed lifecycle projection for the local `workflow_index` read model.
//!
//! This boundary only materializes observations supplied by a caller that has
//! already contacted Temporal. It never reads Temporal or tracker state itself,
//! and its SQLite rows cannot authorize an execution start or lifecycle change.

// This crate-private boundary is deliberately not wired until the separately
// tracked Coordinator integration slice. Its focused tests exercise behavior.
#![allow(dead_code)]

use std::io;

use rusqlite::{
    params_from_iter, types::Type, Connection, ErrorCode, OptionalExtension, Transaction,
};
use sea_query::{Expr, ExprTrait, Query, SqliteQueryBuilder};
use sea_query_rusqlite::RusqliteBinder;
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

use super::{
    Freshness, IssueRef, LocalStateDatabase, LocalStateError, RepoId, WorkflowId,
    WorkflowIndexStatus, WorkspaceRuntimeId,
};

const WORKFLOW_EXECUTION_STEP: &str = "workflow_execution";
const WORKFLOW_CLOSED_STEP: &str = "workflow_closed";
const MAX_START_FAILURE_SUMMARY_BYTES: usize = 1024;

/// A bounded classification supplied when Temporal reports a closed execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkflowCloseStatus {
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
}

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
    fn as_str(self) -> &'static str {
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

/// Stable, bounded code returned for a definitive Temporal start failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitiveStartFailureCode {
    /// Temporal rejected a duplicate Workflow ID according to its start policy.
    AlreadyExists,
    /// Temporal rejected a malformed or incompatible start request.
    InvalidRequest,
    /// Temporal definitively rejected the request for another stable reason.
    Rejected,
}

impl DefinitiveStartFailureCode {
    /// Returns the stable spelling reported to the immediate caller.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyExists => "already_exists",
            Self::InvalidRequest => "invalid_request",
            Self::Rejected => "rejected",
        }
    }
}

/// One caller-supplied Workflow lifecycle observation.
///
/// The immutable activation facts stay directly on each variant so the
/// projector can compare them field by field without inventing a second
/// semantic execution identity beside [`WorkflowId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkflowLifecycleObservation {
    /// A current Temporal Describe observes an open execution.
    DescribeOpen {
        /// Symphony's semantic Temporal Workflow ID.
        workflow_id: WorkflowId,
        /// Stable machine-local runtime scope.
        workspace_runtime_id: WorkspaceRuntimeId,
        /// Repository scope stored with the row.
        repo_id: RepoId,
        /// Tracker-scoped issue reference stored with the row.
        issue_ref: IssueRef,
        /// Tracker state that made this episode executable.
        from_state: String,
        /// Already-known tracker context supplied by the caller.
        current_state: String,
        /// Requested orchestration target for the episode.
        target_kind: String,
        /// Activation provenance reference.
        source_ref: String,
        /// Tracker revision observed at activation.
        source_tracker_revision: String,
        /// Optional operator-action provenance.
        operator_action_ref: Option<String>,
        /// Current Temporal Run ID from Describe evidence.
        run_id: String,
        /// Authoritative execution start time from Describe evidence.
        started_at: OffsetDateTime,
        /// Time at which the caller made this observation.
        observed_at: OffsetDateTime,
    },
    /// A Temporal start response that may confirm, but cannot create, a row.
    StartResponse {
        /// Symphony's semantic Temporal Workflow ID.
        workflow_id: WorkflowId,
        /// Stable machine-local runtime scope.
        workspace_runtime_id: WorkspaceRuntimeId,
        /// Repository scope stored with the row.
        repo_id: RepoId,
        /// Tracker-scoped issue reference stored with the row.
        issue_ref: IssueRef,
        /// Tracker state that made this episode executable.
        from_state: String,
        /// Already-known tracker context supplied by the caller.
        current_state: String,
        /// Requested orchestration target for the episode.
        target_kind: String,
        /// Activation provenance reference.
        source_ref: String,
        /// Tracker revision observed at activation.
        source_tracker_revision: String,
        /// Optional operator-action provenance.
        operator_action_ref: Option<String>,
        /// Run ID returned by Temporal when the response exposes one.
        run_id: Option<String>,
        /// Time at which the caller received the response.
        observed_at: OffsetDateTime,
    },
    /// A definitive start failure that deliberately remains outside v1 storage.
    DefinitiveStartFailure {
        /// Symphony's semantic Temporal Workflow ID.
        workflow_id: WorkflowId,
        /// Stable machine-local runtime scope.
        workspace_runtime_id: WorkspaceRuntimeId,
        /// Repository scope stored with the row.
        repo_id: RepoId,
        /// Tracker-scoped issue reference stored with the row.
        issue_ref: IssueRef,
        /// Tracker state that made this episode executable.
        from_state: String,
        /// Already-known tracker context supplied by the caller.
        current_state: String,
        /// Requested orchestration target for the episode.
        target_kind: String,
        /// Activation provenance reference.
        source_ref: String,
        /// Tracker revision observed at activation.
        source_tracker_revision: String,
        /// Optional operator-action provenance.
        operator_action_ref: Option<String>,
        /// Stable, typed failure code.
        code: DefinitiveStartFailureCode,
        /// Immediate caller summary; it is bounded before being returned.
        summary: String,
        /// Time at which the caller observed the definitive failure.
        observed_at: OffsetDateTime,
    },
    /// A current Temporal Describe observes a closed execution.
    DescribeClosed {
        /// Symphony's semantic Temporal Workflow ID.
        workflow_id: WorkflowId,
        /// Stable machine-local runtime scope.
        workspace_runtime_id: WorkspaceRuntimeId,
        /// Repository scope stored with the row.
        repo_id: RepoId,
        /// Tracker-scoped issue reference stored with the row.
        issue_ref: IssueRef,
        /// Tracker state that made this episode executable.
        from_state: String,
        /// Already-known tracker context supplied by the caller.
        current_state: String,
        /// Requested orchestration target for the episode.
        target_kind: String,
        /// Activation provenance reference.
        source_ref: String,
        /// Tracker revision observed at activation.
        source_tracker_revision: String,
        /// Optional operator-action provenance.
        operator_action_ref: Option<String>,
        /// Current Temporal Run ID from Describe evidence.
        run_id: String,
        /// Authoritative execution start time from Describe evidence.
        started_at: OffsetDateTime,
        /// Supported close classification, when Describe supplies one.
        close_status: Option<WorkflowCloseStatus>,
        /// Optional close-time evidence retained only by this observation.
        closed_at: Option<OffsetDateTime>,
        /// Time at which the caller made this observation.
        observed_at: OffsetDateTime,
    },
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

/// Typed result of applying one lifecycle observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkflowLifecycleProjectionOutcome {
    /// A material lifecycle projection committed and was read back.
    Applied {
        /// Actual row read from the transaction after the material write.
        row: WorkflowIndexRow,
    },
    /// The observation already matches the persisted lifecycle projection.
    AlreadyApplied {
        /// Actual unchanged persisted row.
        row: WorkflowIndexRow,
    },
    /// The observation would regress or contradict the persisted Run chain.
    StaleObservation {
        /// Actual unchanged persisted row.
        row: WorkflowIndexRow,
    },
    /// Immutable activation facts conflict for an existing Workflow ID.
    WorkflowIdentityConflict {
        /// Actual unchanged persisted row.
        row: WorkflowIndexRow,
    },
    /// A different active Workflow already projects the same repository issue.
    ActiveProjectionConflict {
        /// Existing active Symphony Workflow ID from the persisted row.
        existing_workflow_id: WorkflowId,
        /// Actual unchanged active row, including its runtime scope.
        row: WorkflowIndexRow,
    },
    /// A Describe-backed observation is required before SQLite may project it.
    DescribeRequired {
        /// Existing row when a non-matching StartResponse could be read.
        row: Option<WorkflowIndexRow>,
    },
    /// A definitive start failure was deliberately not persisted by v1.
    StartFailureNotProjected {
        /// Stable typed failure code returned to the immediate caller.
        code: DefinitiveStartFailureCode,
        /// Bounded immediate caller summary; never persisted in `workflow_index`.
        summary: String,
    },
}

/// Concrete synchronous writer for Describe-backed `workflow_index` rows.
///
/// This is crate-private because callers must stay behind Symphony's Temporal
/// and tracker boundaries. The projector owns no retry loop, does no network
/// I/O, and cannot reserve or authorize Workflow execution.
#[derive(Debug, Clone)]
pub(crate) struct LocalStateProjector {
    database: LocalStateDatabase,
}

impl LocalStateProjector {
    /// Binds a projector to an already initialized local-state database handle.
    pub(crate) fn new(database: LocalStateDatabase) -> Self {
        Self { database }
    }

    /// Projects one caller-supplied observation through a short SQLite transaction.
    pub(crate) fn project(
        &self,
        observation: WorkflowLifecycleObservation,
    ) -> Result<WorkflowLifecycleProjectionOutcome, LocalStateError> {
        match observation {
            WorkflowLifecycleObservation::DescribeOpen {
                workflow_id,
                workspace_runtime_id,
                repo_id,
                issue_ref,
                from_state,
                current_state,
                target_kind,
                source_ref,
                source_tracker_revision,
                operator_action_ref,
                run_id,
                started_at,
                observed_at,
            } => {
                validate_common_observation(
                    &workflow_id,
                    &workspace_runtime_id,
                    &repo_id,
                    &issue_ref,
                    &from_state,
                    &current_state,
                    &target_kind,
                    &source_ref,
                    &source_tracker_revision,
                    operator_action_ref.as_deref(),
                )?;
                validate_required("run_id", &run_id)?;
                let described_started_at = started_at;
                let started_at = format_timestamp(described_started_at)?;
                let observed_at = format_timestamp(observed_at)?;

                self.database.with_immediate_transaction(|transaction| {
                    let existing = load_workflow(transaction, workflow_id.as_str())?;
                    let Some(row) = existing else {
                        validate_new_row_scope(&repo_id, &issue_ref)
                            .map_err(local_state_error_to_sqlite)?;
                        let proposed = described_row(
                            &workflow_id,
                            &workspace_runtime_id,
                            &repo_id,
                            &issue_ref,
                            &from_state,
                            &current_state,
                            &target_kind,
                            &source_ref,
                            &source_tracker_revision,
                            operator_action_ref.as_deref(),
                            &run_id,
                            &started_at,
                            &observed_at,
                            WorkflowIndexStatus::Running,
                            WORKFLOW_EXECUTION_STEP,
                            None,
                        );
                        return insert_open_or_active_conflict(transaction, proposed);
                    };

                    if !same_immutable_start_facts(
                        &row,
                        &workflow_id,
                        &workspace_runtime_id,
                        &repo_id,
                        &issue_ref,
                        &from_state,
                        &target_kind,
                        &source_ref,
                        &source_tracker_revision,
                        operator_action_ref.as_deref(),
                    ) {
                        return Ok(
                            WorkflowLifecycleProjectionOutcome::WorkflowIdentityConflict { row },
                        );
                    }

                    if row.status != WorkflowIndexStatus::Running {
                        return Ok(WorkflowLifecycleProjectionOutcome::StaleObservation { row });
                    }

                    if row.run_id.as_deref() == Some(run_id.as_str()) {
                        if row.started_at != started_at {
                            return Ok(WorkflowLifecycleProjectionOutcome::StaleObservation {
                                row,
                            });
                        }
                        // `current_state` is caller context, not a tracker read
                        // or local state machine. Same-Run Open evidence cannot
                        // advance it or refresh `updated_at` by itself.
                        return Ok(WorkflowLifecycleProjectionOutcome::AlreadyApplied { row });
                    }

                    let stored_started_at = parse_persisted_timestamp(&row.started_at)?;
                    if described_started_at <= stored_started_at {
                        // Run IDs have no sortable semantics. The authoritative
                        // Describe start time is used only alongside current
                        // Describe evidence to reject an older Run, never as a
                        // standalone lifecycle state machine.
                        return Ok(WorkflowLifecycleProjectionOutcome::StaleObservation { row });
                    }

                    // A DescribeOpen variant represents current-execution
                    // evidence. Only that evidence may advance the stored Run
                    // locator for the same Symphony Workflow ID.
                    let mut replacement = row;
                    replacement.run_id = Some(run_id);
                    replacement.current_state = current_state;
                    replacement.active_step = WORKFLOW_EXECUTION_STEP.to_string();
                    replacement.waiting_kind = None;
                    replacement.started_at = started_at;
                    replacement.status = WorkflowIndexStatus::Running;
                    replacement.terminal_outcome = None;
                    replacement.freshness = Freshness::Fresh;
                    replacement.updated_at = observed_at;
                    update_material_projection(transaction, &replacement)?;
                    readback_applied(transaction, replacement.workflow_id.as_str())
                })
            }
            WorkflowLifecycleObservation::StartResponse {
                workflow_id,
                workspace_runtime_id,
                repo_id,
                issue_ref,
                from_state,
                current_state,
                target_kind,
                source_ref,
                source_tracker_revision,
                operator_action_ref,
                run_id,
                observed_at,
            } => {
                validate_common_observation(
                    &workflow_id,
                    &workspace_runtime_id,
                    &repo_id,
                    &issue_ref,
                    &from_state,
                    &current_state,
                    &target_kind,
                    &source_ref,
                    &source_tracker_revision,
                    operator_action_ref.as_deref(),
                )?;
                let _ = observed_at;
                let run_id = run_id.filter(|value| !value.trim().is_empty());

                self.database.with_immediate_transaction(|transaction| {
                    let Some(row) = load_workflow(transaction, workflow_id.as_str())? else {
                        return Ok(WorkflowLifecycleProjectionOutcome::DescribeRequired {
                            row: None,
                        });
                    };
                    if !same_immutable_start_facts(
                        &row,
                        &workflow_id,
                        &workspace_runtime_id,
                        &repo_id,
                        &issue_ref,
                        &from_state,
                        &target_kind,
                        &source_ref,
                        &source_tracker_revision,
                        operator_action_ref.as_deref(),
                    ) {
                        return Ok(
                            WorkflowLifecycleProjectionOutcome::WorkflowIdentityConflict { row },
                        );
                    }
                    if matches!(
                        run_id.as_deref(),
                        Some(run_id) if row.run_id.as_deref() == Some(run_id)
                    ) {
                        return Ok(WorkflowLifecycleProjectionOutcome::AlreadyApplied { row });
                    }

                    // Start acceptance has no authoritative start timestamp.
                    // It can confirm a projected Run only; it never replaces it.
                    Ok(WorkflowLifecycleProjectionOutcome::DescribeRequired { row: Some(row) })
                })
            }
            WorkflowLifecycleObservation::DefinitiveStartFailure {
                workflow_id,
                workspace_runtime_id,
                repo_id,
                issue_ref,
                from_state,
                current_state,
                target_kind,
                source_ref,
                source_tracker_revision,
                operator_action_ref,
                code,
                summary,
                observed_at,
            } => {
                validate_common_observation(
                    &workflow_id,
                    &workspace_runtime_id,
                    &repo_id,
                    &issue_ref,
                    &from_state,
                    &current_state,
                    &target_kind,
                    &source_ref,
                    &source_tracker_revision,
                    operator_action_ref.as_deref(),
                )?;
                let _ = observed_at;

                // V1 has no bounded failure diagnostic columns. Returning this
                // typed no-write result avoids inventing a Run ID or a start time.
                let outcome = || WorkflowLifecycleProjectionOutcome::StartFailureNotProjected {
                    code,
                    summary: bounded_start_failure_summary(&summary),
                };
                self.database.with_immediate_transaction(|transaction| {
                    let Some(row) = load_workflow(transaction, workflow_id.as_str())? else {
                        return Ok(outcome());
                    };
                    // Failure is not persisted, but it must not conceal an
                    // immutable-fact conflict for an existing Workflow ID.
                    if !same_immutable_start_facts(
                        &row,
                        &workflow_id,
                        &workspace_runtime_id,
                        &repo_id,
                        &issue_ref,
                        &from_state,
                        &target_kind,
                        &source_ref,
                        &source_tracker_revision,
                        operator_action_ref.as_deref(),
                    ) {
                        return Ok(
                            WorkflowLifecycleProjectionOutcome::WorkflowIdentityConflict { row },
                        );
                    }
                    Ok(outcome())
                })
            }
            WorkflowLifecycleObservation::DescribeClosed {
                workflow_id,
                workspace_runtime_id,
                repo_id,
                issue_ref,
                from_state,
                current_state,
                target_kind,
                source_ref,
                source_tracker_revision,
                operator_action_ref,
                run_id,
                started_at,
                close_status,
                closed_at,
                observed_at,
            } => {
                validate_common_observation(
                    &workflow_id,
                    &workspace_runtime_id,
                    &repo_id,
                    &issue_ref,
                    &from_state,
                    &current_state,
                    &target_kind,
                    &source_ref,
                    &source_tracker_revision,
                    operator_action_ref.as_deref(),
                )?;
                validate_required("run_id", &run_id)?;
                let started_at = format_timestamp(started_at)?;
                let observed_at = format_timestamp(observed_at)?;
                // V1 intentionally retains close time as Describe input only;
                // it must not be overloaded into `started_at` or an outcome.
                let _ = closed_at;
                let (status, terminal_outcome) = terminal_projection(close_status);

                self.database.with_immediate_transaction(|transaction| {
                    let existing = load_workflow(transaction, workflow_id.as_str())?;
                    let Some(row) = existing else {
                        validate_new_row_scope(&repo_id, &issue_ref)
                            .map_err(local_state_error_to_sqlite)?;
                        let proposed = described_row(
                            &workflow_id,
                            &workspace_runtime_id,
                            &repo_id,
                            &issue_ref,
                            &from_state,
                            &current_state,
                            &target_kind,
                            &source_ref,
                            &source_tracker_revision,
                            operator_action_ref.as_deref(),
                            &run_id,
                            &started_at,
                            &observed_at,
                            status,
                            WORKFLOW_CLOSED_STEP,
                            Some(terminal_outcome),
                        );
                        insert_workflow(transaction, &proposed)?;
                        return readback_applied(transaction, workflow_id.as_str());
                    };

                    if !same_immutable_start_facts(
                        &row,
                        &workflow_id,
                        &workspace_runtime_id,
                        &repo_id,
                        &issue_ref,
                        &from_state,
                        &target_kind,
                        &source_ref,
                        &source_tracker_revision,
                        operator_action_ref.as_deref(),
                    ) {
                        return Ok(
                            WorkflowLifecycleProjectionOutcome::WorkflowIdentityConflict { row },
                        );
                    }

                    if row.run_id.as_deref() != Some(run_id.as_str())
                        || row.started_at != started_at
                    {
                        // A closed observation for another Run cannot terminate
                        // the newer Run stored under this Workflow ID.
                        return Ok(WorkflowLifecycleProjectionOutcome::StaleObservation { row });
                    }

                    match row.status {
                        WorkflowIndexStatus::Running => {
                            let mut closed = row;
                            closed.current_state = current_state;
                            closed.active_step = WORKFLOW_CLOSED_STEP.to_string();
                            closed.waiting_kind = None;
                            closed.status = status;
                            closed.terminal_outcome = Some(terminal_outcome);
                            closed.freshness = Freshness::Fresh;
                            closed.updated_at = observed_at;
                            update_material_projection(transaction, &closed)?;
                            readback_applied(transaction, closed.workflow_id.as_str())
                        }
                        WorkflowIndexStatus::ClosedUnknown => {
                            if status == WorkflowIndexStatus::ClosedUnknown
                                && row.terminal_outcome == Some(terminal_outcome)
                            {
                                return Ok(WorkflowLifecycleProjectionOutcome::AlreadyApplied {
                                    row,
                                });
                            }

                            let mut refined = row;
                            refined.current_state = current_state;
                            refined.active_step = WORKFLOW_CLOSED_STEP.to_string();
                            refined.waiting_kind = None;
                            refined.status = status;
                            refined.terminal_outcome = Some(terminal_outcome);
                            refined.freshness = Freshness::Fresh;
                            refined.updated_at = observed_at;
                            update_material_projection(transaction, &refined)?;
                            readback_applied(transaction, refined.workflow_id.as_str())
                        }
                        WorkflowIndexStatus::Completed | WorkflowIndexStatus::Failed
                            if row.status == status
                                && row.terminal_outcome == Some(terminal_outcome) =>
                        {
                            Ok(WorkflowLifecycleProjectionOutcome::AlreadyApplied { row })
                        }
                        // Terminal status is monotonic: only the intentionally
                        // unknown terminal class may refine. SQLite never
                        // reopens or reclassifies a completed/failed chain.
                        _ => Ok(WorkflowLifecycleProjectionOutcome::StaleObservation { row }),
                    }
                })
            }
        }
    }
}

// The observation contract intentionally keeps each activation fact direct;
// grouping them here would create the prohibited secondary identity wrapper.
#[allow(clippy::too_many_arguments)]
fn validate_common_observation(
    workflow_id: &WorkflowId,
    workspace_runtime_id: &WorkspaceRuntimeId,
    repo_id: &RepoId,
    issue_ref: &IssueRef,
    from_state: &str,
    current_state: &str,
    target_kind: &str,
    source_ref: &str,
    source_tracker_revision: &str,
    operator_action_ref: Option<&str>,
) -> Result<(), LocalStateError> {
    validate_required("workflow_id", workflow_id.as_str())?;
    validate_required("workspace_runtime_id", workspace_runtime_id.as_str())?;
    validate_required("repo_id.host", &repo_id.host)?;
    validate_required("repo_id.owner", &repo_id.owner)?;
    validate_required("repo_id.repo", &repo_id.repo)?;
    if issue_ref.number == 0 {
        return Err(LocalStateError::ProjectionInput { field: "issue_ref" });
    }
    validate_required("from_state", from_state)?;
    validate_required("current_state", current_state)?;
    validate_required("target_kind", target_kind)?;
    validate_required("source_ref", source_ref)?;
    validate_required("source_tracker_revision", source_tracker_revision)?;
    if let Some(operator_action_ref) = operator_action_ref {
        validate_required("operator_action_ref", operator_action_ref)?;
    }
    Ok(())
}

fn validate_new_row_scope(repo_id: &RepoId, issue_ref: &IssueRef) -> Result<(), LocalStateError> {
    if issue_ref.repo_id != *repo_id {
        return Err(LocalStateError::ProjectionInput {
            field: "repo_id/issue_ref",
        });
    }
    Ok(())
}

fn validate_required(field: &'static str, value: &str) -> Result<(), LocalStateError> {
    if value.trim().is_empty() {
        return Err(LocalStateError::ProjectionInput { field });
    }
    Ok(())
}

fn format_timestamp(value: OffsetDateTime) -> Result<String, LocalStateError> {
    value
        .to_offset(UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(Into::into)
}

fn parse_persisted_timestamp(value: &str) -> rusqlite::Result<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|error| {
        unsupported_storage_value(12, "started_at", &format!("{value:?}: {error}"))
    })
}

fn terminal_projection(
    close_status: Option<WorkflowCloseStatus>,
) -> (WorkflowIndexStatus, WorkflowTerminalOutcome) {
    match close_status {
        Some(WorkflowCloseStatus::Completed) => (
            WorkflowIndexStatus::Completed,
            WorkflowTerminalOutcome::Completed,
        ),
        Some(WorkflowCloseStatus::Failed) => {
            (WorkflowIndexStatus::Failed, WorkflowTerminalOutcome::Failed)
        }
        Some(WorkflowCloseStatus::Cancelled) => (
            WorkflowIndexStatus::Failed,
            WorkflowTerminalOutcome::Cancelled,
        ),
        Some(WorkflowCloseStatus::Terminated) => (
            WorkflowIndexStatus::Failed,
            WorkflowTerminalOutcome::Terminated,
        ),
        Some(WorkflowCloseStatus::TimedOut) => (
            WorkflowIndexStatus::Failed,
            WorkflowTerminalOutcome::TimedOut,
        ),
        None => (
            WorkflowIndexStatus::ClosedUnknown,
            WorkflowTerminalOutcome::ClosedUnknown,
        ),
    }
}

fn bounded_start_failure_summary(summary: &str) -> String {
    if summary.len() <= MAX_START_FAILURE_SUMMARY_BYTES {
        return summary.to_owned();
    }

    let prefix_limit = MAX_START_FAILURE_SUMMARY_BYTES - 3;
    let mut boundary = prefix_limit;
    while !summary.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}...", &summary[..boundary])
}

// V1 maps every persisted fact explicitly so it cannot silently gain an
// identity bundle or fabricate a value absent from Describe evidence.
#[allow(clippy::too_many_arguments)]
fn described_row(
    workflow_id: &WorkflowId,
    workspace_runtime_id: &WorkspaceRuntimeId,
    repo_id: &RepoId,
    issue_ref: &IssueRef,
    from_state: &str,
    current_state: &str,
    target_kind: &str,
    source_ref: &str,
    source_tracker_revision: &str,
    operator_action_ref: Option<&str>,
    run_id: &str,
    started_at: &str,
    observed_at: &str,
    status: WorkflowIndexStatus,
    active_step: &str,
    terminal_outcome: Option<WorkflowTerminalOutcome>,
) -> WorkflowIndexRow {
    WorkflowIndexRow {
        workflow_id: workflow_id.as_str().to_string(),
        run_id: Some(run_id.to_string()),
        workspace_runtime_id: workspace_runtime_id.as_str().to_string(),
        repo_id: repo_id.database_key(),
        issue_ref: issue_ref.database_key(),
        from_state: from_state.to_string(),
        target_kind: target_kind.to_string(),
        current_state: current_state.to_string(),
        active_step: active_step.to_string(),
        waiting_kind: None,
        source_ref: source_ref.to_string(),
        source_tracker_revision: source_tracker_revision.to_string(),
        started_at: started_at.to_string(),
        last_progress_at: None,
        status,
        terminal_outcome,
        operator_action_ref: operator_action_ref.map(str::to_string),
        freshness: Freshness::Fresh,
        updated_at: observed_at.to_string(),
    }
}

// Compare direct fields rather than hiding the immutable activation contract in
// a fingerprint or a second identity type.
#[allow(clippy::too_many_arguments)]
fn same_immutable_start_facts(
    row: &WorkflowIndexRow,
    workflow_id: &WorkflowId,
    workspace_runtime_id: &WorkspaceRuntimeId,
    repo_id: &RepoId,
    issue_ref: &IssueRef,
    from_state: &str,
    target_kind: &str,
    source_ref: &str,
    source_tracker_revision: &str,
    operator_action_ref: Option<&str>,
) -> bool {
    // Compare each immutable activation fact instead of hashing or wrapping
    // them: `workflow_id` remains Symphony's sole semantic execution identity.
    row.workflow_id == workflow_id.as_str()
        && row.workspace_runtime_id == workspace_runtime_id.as_str()
        && row.repo_id == repo_id.database_key()
        && row.issue_ref == issue_ref.database_key()
        && row.from_state == from_state
        && row.target_kind == target_kind
        && row.source_ref == source_ref
        && row.source_tracker_revision == source_tracker_revision
        && row.operator_action_ref.as_deref() == operator_action_ref
}

fn insert_open_or_active_conflict(
    transaction: &Transaction<'_>,
    proposed: WorkflowIndexRow,
) -> rusqlite::Result<WorkflowLifecycleProjectionOutcome> {
    match insert_workflow(transaction, &proposed) {
        Ok(()) => readback_applied(transaction, proposed.workflow_id.as_str()),
        Err(error) if is_constraint_violation(&error) => {
            // The partial index intentionally spans runtime IDs. Query only by
            // repo/issue so a conflicting active projection is never hidden by
            // a workspace scope and never overwritten.
            let Some(row) =
                load_active_workflow(transaction, &proposed.repo_id, &proposed.issue_ref)?
            else {
                return Err(error);
            };
            Ok(
                WorkflowLifecycleProjectionOutcome::ActiveProjectionConflict {
                    existing_workflow_id: WorkflowId::new(row.workflow_id.clone()),
                    row,
                },
            )
        }
        Err(error) => Err(error),
    }
}

fn insert_workflow(connection: &Connection, row: &WorkflowIndexRow) -> rusqlite::Result<()> {
    let (sql, values) = Query::insert()
        .into_table("workflow_index")
        .columns([
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
        ])
        .values_panic([
            row.workflow_id.clone().into(),
            row.run_id.clone().into(),
            row.workspace_runtime_id.clone().into(),
            row.repo_id.clone().into(),
            row.issue_ref.clone().into(),
            row.from_state.clone().into(),
            row.target_kind.clone().into(),
            row.current_state.clone().into(),
            row.active_step.clone().into(),
            row.waiting_kind.clone().into(),
            row.source_ref.clone().into(),
            row.source_tracker_revision.clone().into(),
            row.started_at.clone().into(),
            row.last_progress_at.clone().into(),
            row.status.as_str().into(),
            row.terminal_outcome
                .map(WorkflowTerminalOutcome::as_str)
                .into(),
            row.operator_action_ref.clone().into(),
            row.freshness.as_str().into(),
            row.updated_at.clone().into(),
        ])
        .build_rusqlite(SqliteQueryBuilder);
    connection.execute(&sql, params_from_iter(values.as_params()))?;
    Ok(())
}

fn update_material_projection(
    connection: &Connection,
    row: &WorkflowIndexRow,
) -> rusqlite::Result<()> {
    let (sql, values) = Query::update()
        .table("workflow_index")
        .value("run_id", row.run_id.clone())
        .value("current_state", row.current_state.clone())
        .value("active_step", row.active_step.clone())
        .value("waiting_kind", row.waiting_kind.clone())
        .value("started_at", row.started_at.clone())
        .value("status", row.status.as_str())
        .value(
            "terminal_outcome",
            row.terminal_outcome.map(WorkflowTerminalOutcome::as_str),
        )
        .value("freshness", row.freshness.as_str())
        .value("updated_at", row.updated_at.clone())
        .and_where(Expr::col("workflow_id").eq(row.workflow_id.clone()))
        .build_rusqlite(SqliteQueryBuilder);
    connection.execute(&sql, params_from_iter(values.as_params()))?;
    Ok(())
}

fn readback_applied(
    connection: &Connection,
    workflow_id: &str,
) -> rusqlite::Result<WorkflowLifecycleProjectionOutcome> {
    let row =
        load_workflow(connection, workflow_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    Ok(WorkflowLifecycleProjectionOutcome::Applied { row })
}

fn load_workflow(
    connection: &Connection,
    workflow_id: &str,
) -> rusqlite::Result<Option<WorkflowIndexRow>> {
    let (sql, values) = Query::select()
        .columns(workflow_index_columns())
        .from("workflow_index")
        .and_where(Expr::col("workflow_id").eq(workflow_id))
        .build_rusqlite(SqliteQueryBuilder);
    connection
        .query_row(
            &sql,
            params_from_iter(values.as_params()),
            workflow_index_row,
        )
        .optional()
}

fn load_active_workflow(
    connection: &Connection,
    repo_id: &str,
    issue_ref: &str,
) -> rusqlite::Result<Option<WorkflowIndexRow>> {
    let (sql, values) = Query::select()
        .columns(workflow_index_columns())
        .from("workflow_index")
        .and_where(Expr::col("repo_id").eq(repo_id))
        .and_where(Expr::col("issue_ref").eq(issue_ref))
        .and_where(Expr::col("status").is_in([
            WorkflowIndexStatus::Starting.as_str(),
            WorkflowIndexStatus::Running.as_str(),
        ]))
        .build_rusqlite(SqliteQueryBuilder);
    connection
        .query_row(
            &sql,
            params_from_iter(values.as_params()),
            workflow_index_row,
        )
        .optional()
}

fn workflow_index_columns() -> [&'static str; 19] {
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

fn workflow_index_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowIndexRow> {
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

fn unsupported_storage_value(index: usize, column: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Text,
        Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported workflow_index {column} value {value:?}"),
        )),
    )
}

fn local_state_error_to_sqlite(error: LocalStateError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

fn is_constraint_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == ErrorCode::ConstraintViolation
    )
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use rusqlite::{Connection, TransactionBehavior};
    use tempfile::TempDir;

    use super::*;
    use crate::symphony::local_state::TrackerBackend;

    const BASE_SECONDS: i64 = 1_785_000_000;

    struct Fixture {
        _temporary: TempDir,
        database: LocalStateDatabase,
        projector: LocalStateProjector,
    }

    fn fixture() -> Fixture {
        let temporary = tempfile::tempdir().unwrap();
        let database =
            LocalStateDatabase::at_resolved_path(temporary.path().join("state.db")).unwrap();
        database.initialize().unwrap();
        let projector = LocalStateProjector::new(database.clone());

        Fixture {
            _temporary: temporary,
            database,
            projector,
        }
    }

    fn timestamp(offset: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(BASE_SECONDS + offset).unwrap()
    }

    fn repo() -> RepoId {
        RepoId::new("github.com", "Alive24", "shea-symphony")
    }

    fn issue() -> IssueRef {
        IssueRef::new(TrackerBackend::GithubProjectV2, repo(), 481)
    }

    fn open(
        workflow_id: &str,
        run_id: &str,
        started_at: i64,
        observed_at: i64,
    ) -> WorkflowLifecycleObservation {
        open_with_state(workflow_id, run_id, started_at, observed_at, "In Progress")
    }

    fn open_with_state(
        workflow_id: &str,
        run_id: &str,
        started_at: i64,
        observed_at: i64,
        current_state: &str,
    ) -> WorkflowLifecycleObservation {
        WorkflowLifecycleObservation::DescribeOpen {
            workflow_id: WorkflowId::new(workflow_id),
            workspace_runtime_id: WorkspaceRuntimeId::new("runtime-a"),
            repo_id: repo(),
            issue_ref: issue(),
            from_state: "Todo".to_string(),
            current_state: current_state.to_string(),
            target_kind: "implementation".to_string(),
            source_ref: "project-item:481".to_string(),
            source_tracker_revision: "revision-1".to_string(),
            operator_action_ref: Some("operator-action:481".to_string()),
            run_id: run_id.to_string(),
            started_at: timestamp(started_at),
            observed_at: timestamp(observed_at),
        }
    }

    fn closed(
        workflow_id: &str,
        run_id: &str,
        started_at: i64,
        observed_at: i64,
        close_status: Option<WorkflowCloseStatus>,
    ) -> WorkflowLifecycleObservation {
        WorkflowLifecycleObservation::DescribeClosed {
            workflow_id: WorkflowId::new(workflow_id),
            workspace_runtime_id: WorkspaceRuntimeId::new("runtime-a"),
            repo_id: repo(),
            issue_ref: issue(),
            from_state: "Todo".to_string(),
            current_state: "Agent Review".to_string(),
            target_kind: "implementation".to_string(),
            source_ref: "project-item:481".to_string(),
            source_tracker_revision: "revision-1".to_string(),
            operator_action_ref: Some("operator-action:481".to_string()),
            run_id: run_id.to_string(),
            started_at: timestamp(started_at),
            close_status,
            closed_at: Some(timestamp(observed_at - 1)),
            observed_at: timestamp(observed_at),
        }
    }

    fn start_response(
        workflow_id: &str,
        run_id: Option<&str>,
        observed_at: i64,
    ) -> WorkflowLifecycleObservation {
        WorkflowLifecycleObservation::StartResponse {
            workflow_id: WorkflowId::new(workflow_id),
            workspace_runtime_id: WorkspaceRuntimeId::new("runtime-a"),
            repo_id: repo(),
            issue_ref: issue(),
            from_state: "Todo".to_string(),
            current_state: "In Progress".to_string(),
            target_kind: "implementation".to_string(),
            source_ref: "project-item:481".to_string(),
            source_tracker_revision: "revision-1".to_string(),
            operator_action_ref: Some("operator-action:481".to_string()),
            run_id: run_id.map(str::to_string),
            observed_at: timestamp(observed_at),
        }
    }

    fn start_failure(
        workflow_id: &str,
        summary: &str,
        observed_at: i64,
    ) -> WorkflowLifecycleObservation {
        WorkflowLifecycleObservation::DefinitiveStartFailure {
            workflow_id: WorkflowId::new(workflow_id),
            workspace_runtime_id: WorkspaceRuntimeId::new("runtime-a"),
            repo_id: repo(),
            issue_ref: issue(),
            from_state: "Todo".to_string(),
            current_state: "In Progress".to_string(),
            target_kind: "implementation".to_string(),
            source_ref: "project-item:481".to_string(),
            source_tracker_revision: "revision-1".to_string(),
            operator_action_ref: Some("operator-action:481".to_string()),
            code: DefinitiveStartFailureCode::Rejected,
            summary: summary.to_string(),
            observed_at: timestamp(observed_at),
        }
    }

    fn applied(outcome: WorkflowLifecycleProjectionOutcome) -> WorkflowIndexRow {
        match outcome {
            WorkflowLifecycleProjectionOutcome::Applied { row } => row,
            other => panic!("expected Applied, got {other:?}"),
        }
    }

    fn row_on_disk(database: &LocalStateDatabase, workflow_id: &str) -> Option<WorkflowIndexRow> {
        let connection = Connection::open(database.path()).unwrap();
        load_workflow(&connection, workflow_id).unwrap()
    }

    fn workflow_count(database: &LocalStateDatabase) -> u32 {
        let connection = Connection::open(database.path()).unwrap();
        connection
            .query_row("SELECT COUNT(*) FROM workflow_index", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn describe_open_creates_committed_readback_and_same_run_is_idempotent() {
        let fixture = fixture();
        let row = applied(
            fixture
                .projector
                .project(open("workflow-a", "run-a", 10, 20))
                .unwrap(),
        );

        assert_eq!(row.workflow_id, "workflow-a");
        assert_eq!(row.run_id.as_deref(), Some("run-a"));
        assert_eq!(row.status, WorkflowIndexStatus::Running);
        assert_eq!(row.active_step, WORKFLOW_EXECUTION_STEP);
        assert_eq!(row.waiting_kind, None);
        assert_eq!(row.terminal_outcome, None);
        assert_eq!(row.freshness, Freshness::Fresh);
        assert_eq!(row.last_progress_at, None);
        assert_eq!(row.started_at, format_timestamp(timestamp(10)).unwrap());
        assert_eq!(row.updated_at, format_timestamp(timestamp(20)).unwrap());
        assert_eq!(
            row_on_disk(&fixture.database, "workflow-a"),
            Some(row.clone())
        );

        let duplicate = fixture
            .projector
            .project(open_with_state(
                "workflow-a",
                "run-a",
                10,
                30,
                "Agent Review",
            ))
            .unwrap();
        assert_eq!(
            duplicate,
            WorkflowLifecycleProjectionOutcome::AlreadyApplied { row: row.clone() }
        );
        assert_eq!(row_on_disk(&fixture.database, "workflow-a"), Some(row));
    }

    #[test]
    fn start_response_and_definitive_failure_are_bounded_no_write_outcomes() {
        let fixture = fixture();

        assert_eq!(
            fixture
                .projector
                .project(start_response("workflow-a", Some("run-a"), 10))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::DescribeRequired { row: None }
        );
        assert_eq!(workflow_count(&fixture.database), 0);

        let long_summary = "é".repeat(700);
        let failure = fixture
            .projector
            .project(start_failure("workflow-a", &long_summary, 11))
            .unwrap();
        match failure {
            WorkflowLifecycleProjectionOutcome::StartFailureNotProjected { code, summary } => {
                assert_eq!(code, DefinitiveStartFailureCode::Rejected);
                assert!(summary.len() <= MAX_START_FAILURE_SUMMARY_BYTES);
            }
            other => panic!("expected StartFailureNotProjected, got {other:?}"),
        }
        assert_eq!(workflow_count(&fixture.database), 0);

        let row = applied(
            fixture
                .projector
                .project(open("workflow-a", "run-a", 20, 30))
                .unwrap(),
        );
        assert_eq!(
            fixture
                .projector
                .project(start_response("workflow-a", Some("run-b"), 40))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::DescribeRequired {
                row: Some(row.clone()),
            }
        );
        assert_eq!(
            fixture
                .projector
                .project(start_response("workflow-a", Some("run-a"), 41))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::AlreadyApplied { row: row.clone() }
        );
        assert_eq!(
            fixture
                .projector
                .project(start_response("workflow-a", None, 42))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::DescribeRequired {
                row: Some(row.clone()),
            }
        );
        let _ = fixture
            .projector
            .project(start_failure("workflow-a", "definitive rejection", 43))
            .unwrap();
        assert_eq!(row_on_disk(&fixture.database, "workflow-a"), Some(row));
    }

    #[test]
    fn definitive_start_failure_checks_existing_immutable_facts_without_writing() {
        let fixture = fixture();
        let row = applied(
            fixture
                .projector
                .project(open("workflow-a", "run-a", 10, 20))
                .unwrap(),
        );
        let mut failure = start_failure("workflow-a", "definitive rejection", 30);
        let WorkflowLifecycleObservation::DefinitiveStartFailure { source_ref, .. } = &mut failure
        else {
            unreachable!();
        };
        *source_ref = "other-source".to_string();

        assert_eq!(
            fixture.projector.project(failure).unwrap(),
            WorkflowLifecycleProjectionOutcome::WorkflowIdentityConflict { row: row.clone() }
        );
        assert_eq!(row_on_disk(&fixture.database, "workflow-a"), Some(row));
    }

    #[test]
    fn immutable_start_fact_mismatches_never_mutate_a_matching_workflow_id() {
        let fixture = fixture();
        let original = applied(
            fixture
                .projector
                .project(open("workflow-a", "run-a", 10, 20))
                .unwrap(),
        );

        type Mutator = fn(&mut WorkflowLifecycleObservation);
        let cases: [(&str, Mutator); 8] = [
            ("workspace_runtime_id", mismatch_workspace_runtime_id),
            ("repo_id", mismatch_repo_id),
            ("issue_ref", mismatch_issue_ref),
            ("from_state", mismatch_from_state),
            ("target_kind", mismatch_target_kind),
            ("source_ref", mismatch_source_ref),
            ("source_tracker_revision", mismatch_source_tracker_revision),
            ("operator_action_ref", mismatch_operator_action_ref),
        ];

        for (field, mutate) in cases {
            let mut candidate = open("workflow-a", "run-a", 10, 30);
            mutate(&mut candidate);
            let outcome = fixture.projector.project(candidate).unwrap();
            assert_eq!(
                outcome,
                WorkflowLifecycleProjectionOutcome::WorkflowIdentityConflict {
                    row: original.clone(),
                },
                "{field} must be an immutable identity conflict"
            );
            assert_eq!(
                row_on_disk(&fixture.database, "workflow-a"),
                Some(original.clone()),
                "{field} must not mutate the row"
            );
        }
    }

    #[test]
    fn current_describe_can_replace_a_run_but_old_run_evidence_cannot_close_it() {
        let fixture = fixture();
        let _ = applied(
            fixture
                .projector
                .project(open("workflow-a", "run-a", 10, 20))
                .unwrap(),
        );
        let replacement = applied(
            fixture
                .projector
                .project(open("workflow-a", "run-b", 30, 40))
                .unwrap(),
        );
        assert_eq!(replacement.run_id.as_deref(), Some("run-b"));
        assert_eq!(
            replacement.started_at,
            format_timestamp(timestamp(30)).unwrap()
        );
        assert_eq!(
            replacement.updated_at,
            format_timestamp(timestamp(40)).unwrap()
        );

        assert_eq!(
            fixture
                .projector
                .project(open("workflow-a", "run-a", 10, 45))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::StaleObservation {
                row: replacement.clone(),
            }
        );
        assert_eq!(
            fixture
                .projector
                .project(closed(
                    "workflow-a",
                    "run-a",
                    10,
                    50,
                    Some(WorkflowCloseStatus::Completed),
                ))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::StaleObservation {
                row: replacement.clone(),
            }
        );
        assert_eq!(
            fixture
                .projector
                .project(start_response("workflow-a", Some("run-a"), 51))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::DescribeRequired {
                row: Some(replacement.clone()),
            }
        );

        let completed = applied(
            fixture
                .projector
                .project(closed(
                    "workflow-a",
                    "run-b",
                    30,
                    60,
                    Some(WorkflowCloseStatus::Completed),
                ))
                .unwrap(),
        );
        assert_eq!(completed.status, WorkflowIndexStatus::Completed);
        assert_eq!(
            completed.terminal_outcome,
            Some(WorkflowTerminalOutcome::Completed)
        );
        assert_eq!(completed.active_step, WORKFLOW_CLOSED_STEP);
        assert_eq!(
            completed.updated_at,
            format_timestamp(timestamp(60)).unwrap()
        );
    }

    #[test]
    fn closed_unknown_refines_once_and_terminal_states_are_monotonic() {
        let fixture = fixture();
        let unknown = applied(
            fixture
                .projector
                .project(closed("workflow-a", "run-a", 10, 20, None))
                .unwrap(),
        );
        assert_eq!(unknown.status, WorkflowIndexStatus::ClosedUnknown);
        assert_eq!(
            unknown.terminal_outcome,
            Some(WorkflowTerminalOutcome::ClosedUnknown)
        );

        let completed = applied(
            fixture
                .projector
                .project(closed(
                    "workflow-a",
                    "run-a",
                    10,
                    30,
                    Some(WorkflowCloseStatus::Completed),
                ))
                .unwrap(),
        );
        assert_eq!(completed.status, WorkflowIndexStatus::Completed);
        assert_eq!(
            fixture
                .projector
                .project(closed(
                    "workflow-a",
                    "run-a",
                    10,
                    40,
                    Some(WorkflowCloseStatus::Completed),
                ))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::AlreadyApplied {
                row: completed.clone(),
            }
        );
        assert_eq!(
            fixture
                .projector
                .project(closed(
                    "workflow-a",
                    "run-a",
                    10,
                    50,
                    Some(WorkflowCloseStatus::Failed),
                ))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::StaleObservation {
                row: completed.clone(),
            }
        );
        assert_eq!(
            fixture
                .projector
                .project(open("workflow-a", "run-a", 10, 60))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::StaleObservation { row: completed }
        );

        let failed = applied(
            fixture
                .projector
                .project(closed(
                    "workflow-b",
                    "run-b",
                    70,
                    80,
                    Some(WorkflowCloseStatus::Failed),
                ))
                .unwrap(),
        );
        assert_eq!(
            fixture
                .projector
                .project(closed(
                    "workflow-b",
                    "run-b",
                    70,
                    90,
                    Some(WorkflowCloseStatus::Cancelled),
                ))
                .unwrap(),
            WorkflowLifecycleProjectionOutcome::StaleObservation { row: failed }
        );
    }

    #[test]
    fn active_projection_conflicts_remain_machine_wide_and_terminal_rows_do_not_block() {
        let fixture = fixture();
        let active = applied(
            fixture
                .projector
                .project(open("workflow-a", "run-a", 10, 20))
                .unwrap(),
        );
        let mut another_runtime = open("workflow-b", "run-b", 30, 40);
        if let WorkflowLifecycleObservation::DescribeOpen {
            workspace_runtime_id,
            ..
        } = &mut another_runtime
        {
            *workspace_runtime_id = WorkspaceRuntimeId::new("runtime-b");
        }

        assert_eq!(
            fixture.projector.project(another_runtime.clone()).unwrap(),
            WorkflowLifecycleProjectionOutcome::ActiveProjectionConflict {
                existing_workflow_id: WorkflowId::new("workflow-a"),
                row: active.clone(),
            }
        );

        let _ = applied(
            fixture
                .projector
                .project(closed(
                    "workflow-a",
                    "run-a",
                    10,
                    50,
                    Some(WorkflowCloseStatus::Completed),
                ))
                .unwrap(),
        );
        let replacement = applied(fixture.projector.project(another_runtime).unwrap());
        assert_eq!(replacement.workflow_id, "workflow-b");
        assert_eq!(replacement.workspace_runtime_id, "runtime-b");
        assert_eq!(replacement.status, WorkflowIndexStatus::Running);
    }

    #[test]
    fn injected_transaction_failure_rolls_back_the_projection_write() {
        let fixture = fixture();
        let connection = Connection::open(fixture.database.path()).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_projector_insert \
                 BEFORE INSERT ON workflow_index \
                 BEGIN SELECT RAISE(ROLLBACK, 'injected projection failure'); END;",
            )
            .unwrap();

        assert!(matches!(
            fixture
                .projector
                .project(open("workflow-a", "run-a", 10, 20)),
            Err(LocalStateError::Projection { .. })
        ));
        assert_eq!(workflow_count(&fixture.database), 0);
        assert_eq!(row_on_disk(&fixture.database, "workflow-a"), None);
    }

    #[test]
    fn bounded_lock_contention_returns_database_busy_without_an_internal_retry_loop() {
        let fixture = fixture();
        let mut lock_connection = Connection::open(fixture.database.path()).unwrap();
        let lock = lock_connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();

        let started = Instant::now();
        assert!(matches!(
            fixture
                .projector
                .project(open("workflow-a", "run-a", 10, 20)),
            Err(LocalStateError::DatabaseBusy { .. })
        ));
        assert!(started.elapsed() >= Duration::from_secs(4));
        assert!(started.elapsed() < Duration::from_secs(7));
        drop(lock);
    }

    fn mismatch_workspace_runtime_id(observation: &mut WorkflowLifecycleObservation) {
        let WorkflowLifecycleObservation::DescribeOpen {
            workspace_runtime_id,
            ..
        } = observation
        else {
            unreachable!();
        };
        *workspace_runtime_id = WorkspaceRuntimeId::new("runtime-b");
    }

    fn mismatch_repo_id(observation: &mut WorkflowLifecycleObservation) {
        let WorkflowLifecycleObservation::DescribeOpen { repo_id, .. } = observation else {
            unreachable!();
        };
        *repo_id = RepoId::new("github.com", "Alive24", "other-repository");
    }

    fn mismatch_issue_ref(observation: &mut WorkflowLifecycleObservation) {
        let WorkflowLifecycleObservation::DescribeOpen { issue_ref, .. } = observation else {
            unreachable!();
        };
        *issue_ref = IssueRef::new(TrackerBackend::GithubProjectV2, repo(), 482);
    }

    fn mismatch_from_state(observation: &mut WorkflowLifecycleObservation) {
        let WorkflowLifecycleObservation::DescribeOpen { from_state, .. } = observation else {
            unreachable!();
        };
        *from_state = "Rework".to_string();
    }

    fn mismatch_target_kind(observation: &mut WorkflowLifecycleObservation) {
        let WorkflowLifecycleObservation::DescribeOpen { target_kind, .. } = observation else {
            unreachable!();
        };
        *target_kind = "agent_review".to_string();
    }

    fn mismatch_source_ref(observation: &mut WorkflowLifecycleObservation) {
        let WorkflowLifecycleObservation::DescribeOpen { source_ref, .. } = observation else {
            unreachable!();
        };
        *source_ref = "project-item:other".to_string();
    }

    fn mismatch_source_tracker_revision(observation: &mut WorkflowLifecycleObservation) {
        let WorkflowLifecycleObservation::DescribeOpen {
            source_tracker_revision,
            ..
        } = observation
        else {
            unreachable!();
        };
        *source_tracker_revision = "revision-2".to_string();
    }

    fn mismatch_operator_action_ref(observation: &mut WorkflowLifecycleObservation) {
        let WorkflowLifecycleObservation::DescribeOpen {
            operator_action_ref,
            ..
        } = observation
        else {
            unreachable!();
        };
        *operator_action_ref = None;
    }
}
