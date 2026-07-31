//! Targeted, Describe-backed Coordinator reconciliation.
//!
//! A repair consumes one existing executable activation, reads only that
//! issue's local active binding, and describes the activation's exact Workflow
//! ID once. Temporal remains the execution authority; SQLite is updated only
//! through [`LocalStateProjector`] after current Describe evidence validates.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

use thiserror::Error;
use time::OffsetDateTime;

use super::{
    start::{
        normalize_current_describe, CoordinatorExecutionObservation, CoordinatorFailureKind,
        CoordinatorSdkErrorVariant, CoordinatorStartFailure, CoordinatorTemporalAdapter,
        CoordinatorTemporalStatus,
    },
    CoordinatorExecutableActivation,
};
use crate::symphony::local_state::{
    projector::{
        LocalStateProjector, WorkflowCloseStatus, WorkflowLifecycleObservation,
        WorkflowLifecycleProjectionOutcome,
    },
    reader::{LocalStateReader, LocalStateReaderError},
    LocalStateError, WorkflowId, WorkflowIndexRow, WorkspaceRuntimeId,
};

/// A bounded request to reconcile one exact Coordinator execution identity.
///
/// The request accepts [`CoordinatorExecutableActivation`] rather than loose
/// strings, preserving the identity and immutable activation facts already
/// validated by the Coordinator contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoordinatorReconciliationRequest {
    activation: CoordinatorExecutableActivation,
    workspace_runtime_id: WorkspaceRuntimeId,
    observed_at: OffsetDateTime,
}

impl CoordinatorReconciliationRequest {
    /// Builds a request without reading tracker, SQLite, or Temporal state.
    pub(crate) fn new(
        activation: CoordinatorExecutableActivation,
        workspace_runtime_id: WorkspaceRuntimeId,
        observed_at: OffsetDateTime,
    ) -> Result<Self, CoordinatorReconciliationRequestError> {
        if workspace_runtime_id.as_str().trim().is_empty() {
            return Err(CoordinatorReconciliationRequestError::EmptyWorkspaceRuntimeId);
        }

        Ok(Self {
            activation,
            workspace_runtime_id,
            observed_at,
        })
    }

    /// Borrows the validated activation that owns the exact Workflow ID.
    pub(crate) const fn activation(&self) -> &CoordinatorExecutableActivation {
        &self.activation
    }

    /// Borrows the machine-local runtime scope used by the projection.
    pub(crate) const fn workspace_runtime_id(&self) -> &WorkspaceRuntimeId {
        &self.workspace_runtime_id
    }

    /// Returns the time at which this repair observes Temporal.
    pub(crate) const fn observed_at(&self) -> OffsetDateTime {
        self.observed_at
    }
}

/// Input validation failure for a targeted reconciliation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum CoordinatorReconciliationRequestError {
    /// A stable runtime scope is required before a projection can be written.
    #[error("workspace runtime ID must not be empty")]
    EmptyWorkspaceRuntimeId,
}

/// Result of one targeted repair invocation.
#[derive(Debug)]
pub(crate) struct CoordinatorReconciliationResult {
    /// Exact Coordinator identity requested for repair.
    pub(crate) workflow_id: WorkflowId,
    /// Applied, idempotent, or typed no-write repair outcome.
    pub(crate) outcome: CoordinatorReconciliationOutcome,
}

/// Observable result for a targeted reconciliation invocation.
#[derive(Debug)]
pub(crate) enum CoordinatorReconciliationOutcome {
    /// Current Describe evidence caused a material local lifecycle projection.
    Applied {
        /// The one active binding read before projection, when present.
        local_active_binding: Option<WorkflowIndexRow>,
        /// Committed row read back from the projection transaction.
        row: WorkflowIndexRow,
    },
    /// Current Describe evidence already matches the local lifecycle projection.
    AlreadyApplied {
        /// The one active binding read before projection, when present.
        local_active_binding: Option<WorkflowIndexRow>,
        /// Unchanged existing projection row.
        row: WorkflowIndexRow,
    },
    /// No local lifecycle write was made because evidence or projection was insufficient.
    NoWrite(CoordinatorReconciliationNoWriteOutcome),
}

/// Typed, inspectable outcome for a targeted repair that made no local write.
#[derive(Debug)]
pub(crate) enum CoordinatorReconciliationNoWriteOutcome {
    /// Temporal did not find the requested execution; existing local data remains intact.
    TemporalNotFound {
        /// Bounded Describe failure evidence.
        failure: CoordinatorStartFailure,
    },
    /// Temporal could not supply a current Describe observation.
    TemporalUnavailable {
        /// Bounded Describe failure evidence.
        failure: CoordinatorStartFailure,
    },
    /// Describe returned malformed or contradictory evidence.
    MalformedDescribe {
        /// Bounded Describe validation failure.
        failure: CoordinatorStartFailure,
    },
    /// Temporal definitively rejected Describe without supplying usable evidence.
    TemporalRejected {
        /// Bounded Describe failure evidence.
        failure: CoordinatorStartFailure,
    },
    /// The one scoped local-binding read could not complete.
    LocalBindingUnavailable {
        /// Typed local reader failure.
        failure: LocalStateReaderError,
    },
    /// SQLite could not complete the transactional projection.
    ProjectionUnavailable {
        /// Typed local-state projection failure.
        failure: LocalStateError,
    },
    /// The projector retained an existing row because its evidence conflicts or regresses.
    ProjectionConflict(Box<CoordinatorProjectionConflict>),
}

/// Existing local projection evidence that prevented a targeted repair write.
#[derive(Debug)]
pub(crate) struct CoordinatorProjectionConflict {
    /// The one active binding read before attempting projection, when present.
    pub(crate) local_active_binding: Option<WorkflowIndexRow>,
    /// Existing typed projector no-write outcome.
    pub(crate) outcome: WorkflowLifecycleProjectionOutcome,
}

/// Reconciles one exact activation using one current, unpinned Temporal Describe.
///
/// This function never starts, retries, terminates, or replaces a Workflow.
/// It performs no tracker I/O. A local Run ID is read only as diagnostic state
/// and is intentionally not supplied to Describe, so current evidence can
/// repair it when Temporal reports a newer Run.
pub(crate) async fn reconcile_current_execution<A: CoordinatorTemporalAdapter>(
    adapter: &A,
    reader: &LocalStateReader,
    projector: &LocalStateProjector,
    request: CoordinatorReconciliationRequest,
) -> CoordinatorReconciliationResult {
    let workflow_id = request.activation.workflow_id().clone();
    let described = match adapter.describe_issue_workflow(&workflow_id, None).await {
        Ok(evidence) => evidence,
        Err(failure) => {
            return CoordinatorReconciliationResult {
                workflow_id,
                outcome: CoordinatorReconciliationOutcome::NoWrite(classify_describe_failure(
                    failure,
                )),
            };
        }
    };
    let execution = match normalize_current_describe(&workflow_id, None, described) {
        Ok(execution) => execution,
        Err(failure) => {
            return CoordinatorReconciliationResult {
                workflow_id,
                outcome: CoordinatorReconciliationOutcome::NoWrite(
                    CoordinatorReconciliationNoWriteOutcome::MalformedDescribe { failure },
                ),
            };
        }
    };

    let local_active_binding = match reader.find_active_workflow_for_issue(
        &request.activation.issue_ref().repo_id,
        request.activation.issue_ref(),
    ) {
        Ok(binding) => binding,
        Err(failure) => {
            return CoordinatorReconciliationResult {
                workflow_id,
                outcome: CoordinatorReconciliationOutcome::NoWrite(
                    CoordinatorReconciliationNoWriteOutcome::LocalBindingUnavailable { failure },
                ),
            };
        }
    };

    match projector.project(lifecycle_observation(&request, execution)) {
        Ok(WorkflowLifecycleProjectionOutcome::Applied { row }) => {
            CoordinatorReconciliationResult {
                workflow_id,
                outcome: CoordinatorReconciliationOutcome::Applied {
                    local_active_binding,
                    row,
                },
            }
        }
        Ok(WorkflowLifecycleProjectionOutcome::AlreadyApplied { row }) => {
            CoordinatorReconciliationResult {
                workflow_id,
                outcome: CoordinatorReconciliationOutcome::AlreadyApplied {
                    local_active_binding,
                    row,
                },
            }
        }
        Ok(outcome) => CoordinatorReconciliationResult {
            workflow_id,
            outcome: CoordinatorReconciliationOutcome::NoWrite(
                CoordinatorReconciliationNoWriteOutcome::ProjectionConflict(Box::new(
                    CoordinatorProjectionConflict {
                        local_active_binding,
                        outcome,
                    },
                )),
            ),
        },
        Err(failure) => CoordinatorReconciliationResult {
            workflow_id,
            outcome: CoordinatorReconciliationOutcome::NoWrite(
                CoordinatorReconciliationNoWriteOutcome::ProjectionUnavailable { failure },
            ),
        },
    }
}

fn classify_describe_failure(
    failure: CoordinatorStartFailure,
) -> CoordinatorReconciliationNoWriteOutcome {
    match failure.sdk_error_variant() {
        CoordinatorSdkErrorVariant::NotFound => {
            CoordinatorReconciliationNoWriteOutcome::TemporalNotFound { failure }
        }
        CoordinatorSdkErrorVariant::EvidenceValidation
        | CoordinatorSdkErrorVariant::PayloadConversion => {
            CoordinatorReconciliationNoWriteOutcome::MalformedDescribe { failure }
        }
        _ if failure.kind() == CoordinatorFailureKind::DefinitiveServerRejection => {
            CoordinatorReconciliationNoWriteOutcome::TemporalRejected { failure }
        }
        _ => CoordinatorReconciliationNoWriteOutcome::TemporalUnavailable { failure },
    }
}

fn lifecycle_observation(
    request: &CoordinatorReconciliationRequest,
    execution: CoordinatorExecutionObservation,
) -> WorkflowLifecycleObservation {
    let activation = request.activation();

    match execution {
        CoordinatorExecutionObservation::Open {
            run_id,
            temporal_started_at,
            status,
            ..
        } => {
            debug_assert!(matches!(
                status,
                CoordinatorTemporalStatus::Running | CoordinatorTemporalStatus::Paused
            ));
            WorkflowLifecycleObservation::DescribeOpen {
                workflow_id: activation.workflow_id().clone(),
                workspace_runtime_id: request.workspace_runtime_id().clone(),
                repo_id: activation.issue_ref().repo_id.clone(),
                issue_ref: activation.issue_ref().clone(),
                from_state: activation.observed().state().as_str().to_owned(),
                current_state: activation.observed().state().as_str().to_owned(),
                target_kind: activation.target_kind().as_str().to_owned(),
                source_ref: activation.source_ref().to_owned(),
                source_tracker_revision: activation.observed().revision().to_owned(),
                operator_action_ref: (activation.source_kind()
                    == super::CoordinatorSourceKind::OperatorAction)
                    .then(|| activation.source_ref().to_owned()),
                run_id,
                started_at: temporal_started_at,
                observed_at: request.observed_at(),
            }
        }
        CoordinatorExecutionObservation::Closed {
            run_id,
            temporal_started_at,
            status,
            ..
        } => WorkflowLifecycleObservation::DescribeClosed {
            workflow_id: activation.workflow_id().clone(),
            workspace_runtime_id: request.workspace_runtime_id().clone(),
            repo_id: activation.issue_ref().repo_id.clone(),
            issue_ref: activation.issue_ref().clone(),
            from_state: activation.observed().state().as_str().to_owned(),
            current_state: activation.observed().state().as_str().to_owned(),
            target_kind: activation.target_kind().as_str().to_owned(),
            source_ref: activation.source_ref().to_owned(),
            source_tracker_revision: activation.observed().revision().to_owned(),
            operator_action_ref: (activation.source_kind()
                == super::CoordinatorSourceKind::OperatorAction)
                .then(|| activation.source_ref().to_owned()),
            run_id,
            started_at: temporal_started_at,
            close_status: close_status(status),
            closed_at: None,
            observed_at: request.observed_at(),
        },
        CoordinatorExecutionObservation::DescribeRequired => {
            unreachable!("validated current Describe cannot require another Describe")
        }
    }
}

fn close_status(status: CoordinatorTemporalStatus) -> Option<WorkflowCloseStatus> {
    match status {
        CoordinatorTemporalStatus::Completed => Some(WorkflowCloseStatus::Completed),
        CoordinatorTemporalStatus::Failed => Some(WorkflowCloseStatus::Failed),
        CoordinatorTemporalStatus::Canceled => Some(WorkflowCloseStatus::Cancelled),
        CoordinatorTemporalStatus::Terminated => Some(WorkflowCloseStatus::Terminated),
        CoordinatorTemporalStatus::TimedOut => Some(WorkflowCloseStatus::TimedOut),
        CoordinatorTemporalStatus::ContinuedAsNew => None,
        CoordinatorTemporalStatus::Running | CoordinatorTemporalStatus::Paused => {
            unreachable!("open Temporal statuses cannot form a closed observation")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use time::macros::datetime;

    use super::*;
    use crate::symphony::{
        coordinator::{
            start::{
                CoordinatorAdapterStart, CoordinatorDescribeEvidence, CoordinatorTemporalPhase,
            },
            CoordinatorActivationDecision, CoordinatorActivationRequest, CoordinatorSourceKind,
            CoordinatorTrackerState, ObservedTrackerSnapshot,
        },
        local_state::{LocalStateDatabase, TrackerBackend},
    };

    struct Fixture {
        _temporary: tempfile::TempDir,
        reader: LocalStateReader,
        projector: LocalStateProjector,
    }

    impl Fixture {
        fn new() -> Self {
            let temporary = tempfile::tempdir().unwrap();
            let database =
                LocalStateDatabase::at_resolved_path(temporary.path().join("state.db")).unwrap();
            database.initialize().unwrap();
            Self {
                _temporary: temporary,
                reader: LocalStateReader::new(database.clone()),
                projector: LocalStateProjector::new(database),
            }
        }
    }

    struct FakeAdapter {
        describes: Mutex<VecDeque<Result<CoordinatorDescribeEvidence, CoordinatorStartFailure>>>,
        describe_calls: Mutex<Vec<(WorkflowId, Option<String>)>>,
    }

    impl FakeAdapter {
        fn new(
            describes: impl IntoIterator<
                Item = Result<CoordinatorDescribeEvidence, CoordinatorStartFailure>,
            >,
        ) -> Self {
            Self {
                describes: Mutex::new(describes.into_iter().collect()),
                describe_calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl CoordinatorTemporalAdapter for FakeAdapter {
        async fn start_issue_workflow(
            &self,
            _input: crate::symphony::dto::IssueWorkflowInput,
        ) -> Result<CoordinatorAdapterStart, CoordinatorStartFailure> {
            unreachable!("targeted repair must not start a Workflow")
        }

        async fn describe_issue_workflow(
            &self,
            workflow_id: &WorkflowId,
            run_id: Option<&str>,
        ) -> Result<CoordinatorDescribeEvidence, CoordinatorStartFailure> {
            self.describe_calls
                .lock()
                .unwrap()
                .push((workflow_id.clone(), run_id.map(str::to_owned)));
            self.describes.lock().unwrap().pop_front().unwrap()
        }
    }

    fn activation() -> CoordinatorExecutableActivation {
        let repo_id = crate::symphony::RepoId::new("github.com", "Alive24", "shea-symphony");
        let issue_ref =
            crate::symphony::IssueRef::new(TrackerBackend::GithubProjectV2, repo_id, 504);
        let request = CoordinatorActivationRequest::new(
            issue_ref,
            Some(CoordinatorTrackerState::Todo),
            Some("revision-504".to_string()),
            datetime!(2026-07-31 19:20:00 UTC),
            CoordinatorSourceKind::Reconciliation,
            "project-item:504",
            "repair local binding from Temporal Describe",
        )
        .unwrap();
        let observed =
            ObservedTrackerSnapshot::new(CoordinatorTrackerState::Todo, "revision-504").unwrap();
        let CoordinatorActivationDecision::Executable(activation) =
            request.evaluate(&observed).unwrap()
        else {
            panic!("Todo must be executable");
        };
        activation
    }

    fn repair_request(
        activation: &CoordinatorExecutableActivation,
    ) -> CoordinatorReconciliationRequest {
        CoordinatorReconciliationRequest::new(
            activation.clone(),
            WorkspaceRuntimeId::new("runtime-504"),
            datetime!(2026-07-31 19:21:00 UTC),
        )
        .unwrap()
    }

    fn describe(
        activation: &CoordinatorExecutableActivation,
        run_id: &str,
        started_at: OffsetDateTime,
        status: CoordinatorTemporalStatus,
    ) -> CoordinatorDescribeEvidence {
        CoordinatorDescribeEvidence {
            workflow_id: activation.workflow_id().as_str().to_owned(),
            run_id: run_id.to_owned(),
            temporal_started_at: Some(started_at),
            status: Some(status),
        }
    }

    fn unavailable(
        activation: &CoordinatorExecutableActivation,
        variant: CoordinatorSdkErrorVariant,
    ) -> CoordinatorStartFailure {
        CoordinatorStartFailure::new(
            CoordinatorTemporalPhase::Describe,
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            activation.workflow_id().clone(),
            None,
            variant,
            None,
        )
    }

    #[tokio::test]
    async fn missing_row_is_repaired_from_one_current_open_describe() {
        let fixture = Fixture::new();
        let activation = activation();
        let adapter = FakeAdapter::new([Ok(describe(
            &activation,
            "run-current",
            datetime!(2026-07-31 19:00:00 UTC),
            CoordinatorTemporalStatus::Running,
        ))]);

        let result = reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;

        let CoordinatorReconciliationOutcome::Applied { row, .. } = result.outcome else {
            panic!("expected a material projection");
        };
        assert_eq!(row.run_id.as_deref(), Some("run-current"));
        assert_eq!(row.status, crate::symphony::WorkflowIndexStatus::Running);
        assert_eq!(
            adapter.describe_calls.lock().unwrap().as_slice(),
            &[(activation.workflow_id().clone(), None)]
        );
    }

    #[tokio::test]
    async fn existing_projection_is_confirmed_idempotently() {
        let fixture = Fixture::new();
        let activation = activation();
        let evidence = describe(
            &activation,
            "run-current",
            datetime!(2026-07-31 19:00:00 UTC),
            CoordinatorTemporalStatus::Running,
        );
        let adapter = FakeAdapter::new([Ok(evidence.clone()), Ok(evidence)]);

        let first = reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;
        assert!(matches!(
            first.outcome,
            CoordinatorReconciliationOutcome::Applied { .. }
        ));
        let second = reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;
        assert!(matches!(
            second.outcome,
            CoordinatorReconciliationOutcome::AlreadyApplied { .. }
        ));
    }

    #[tokio::test]
    async fn newer_current_run_replaces_stale_local_run() {
        let fixture = Fixture::new();
        let activation = activation();
        let adapter = FakeAdapter::new([
            Ok(describe(
                &activation,
                "run-old",
                datetime!(2026-07-31 19:00:00 UTC),
                CoordinatorTemporalStatus::Running,
            )),
            Ok(describe(
                &activation,
                "run-new",
                datetime!(2026-07-31 19:01:00 UTC),
                CoordinatorTemporalStatus::Running,
            )),
        ]);

        reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;
        let repaired = reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;

        let CoordinatorReconciliationOutcome::Applied { row, .. } = repaired.outcome else {
            panic!("newer current Run must repair the local binding");
        };
        assert_eq!(row.run_id.as_deref(), Some("run-new"));
        assert_eq!(adapter.describe_calls.lock().unwrap()[1].1, None);
    }

    #[tokio::test]
    async fn closed_describe_projects_supported_terminal_status() {
        let fixture = Fixture::new();
        let activation = activation();
        let adapter = FakeAdapter::new([Ok(describe(
            &activation,
            "run-closed",
            datetime!(2026-07-31 19:00:00 UTC),
            CoordinatorTemporalStatus::Completed,
        ))]);

        let result = reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;

        let CoordinatorReconciliationOutcome::Applied { row, .. } = result.outcome else {
            panic!("closed Describe must project a terminal row");
        };
        assert_eq!(row.status, crate::symphony::WorkflowIndexStatus::Completed);
    }

    #[tokio::test]
    async fn conflicting_active_binding_is_diagnostic_and_preserved() {
        let fixture = Fixture::new();
        let activation = activation();
        let conflicting = WorkflowLifecycleObservation::DescribeOpen {
            workflow_id: WorkflowId::new("issue:other-active-workflow"),
            workspace_runtime_id: WorkspaceRuntimeId::new("runtime-other"),
            repo_id: activation.issue_ref().repo_id.clone(),
            issue_ref: activation.issue_ref().clone(),
            from_state: "todo".to_string(),
            current_state: "todo".to_string(),
            target_kind: "work".to_string(),
            source_ref: "project-item:other".to_string(),
            source_tracker_revision: "revision-other".to_string(),
            operator_action_ref: None,
            run_id: "run-other".to_string(),
            started_at: datetime!(2026-07-31 18:00:00 UTC),
            observed_at: datetime!(2026-07-31 18:01:00 UTC),
        };
        fixture.projector.project(conflicting).unwrap();
        let adapter = FakeAdapter::new([Ok(describe(
            &activation,
            "run-target",
            datetime!(2026-07-31 19:00:00 UTC),
            CoordinatorTemporalStatus::Running,
        ))]);

        let result = reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;

        let CoordinatorReconciliationOutcome::NoWrite(
            CoordinatorReconciliationNoWriteOutcome::ProjectionConflict(conflict),
        ) = result.outcome
        else {
            panic!("local active conflict must remain diagnostic no-write evidence");
        };
        let Some(binding) = conflict.local_active_binding else {
            panic!("the scoped local binding should be retained as diagnostic evidence");
        };
        let WorkflowLifecycleProjectionOutcome::ActiveProjectionConflict { row, .. } =
            conflict.outcome
        else {
            panic!("expected the projector's typed active conflict");
        };
        assert_eq!(binding.workflow_id, "issue:other-active-workflow");
        assert_eq!(row.workflow_id, "issue:other-active-workflow");
    }

    #[tokio::test]
    async fn malformed_describe_preserves_existing_projection() {
        let fixture = Fixture::new();
        let activation = activation();
        let valid = describe(
            &activation,
            "run-current",
            datetime!(2026-07-31 19:00:00 UTC),
            CoordinatorTemporalStatus::Running,
        );
        let malformed = CoordinatorDescribeEvidence {
            status: None,
            ..valid.clone()
        };
        let adapter = FakeAdapter::new([Ok(valid), Ok(malformed)]);

        reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;
        let result = reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;

        assert!(matches!(
            result.outcome,
            CoordinatorReconciliationOutcome::NoWrite(
                CoordinatorReconciliationNoWriteOutcome::MalformedDescribe { .. }
            )
        ));
        let row = fixture
            .reader
            .find_active_workflow_for_issue(&activation.issue_ref().repo_id, activation.issue_ref())
            .unwrap()
            .unwrap();
        assert_eq!(row.run_id.as_deref(), Some("run-current"));
    }

    #[tokio::test]
    async fn unavailable_and_not_found_describe_are_typed_no_write_outcomes() {
        let fixture = Fixture::new();
        let activation = activation();
        let adapter = FakeAdapter::new([
            Err(unavailable(&activation, CoordinatorSdkErrorVariant::Rpc)),
            Err(unavailable(
                &activation,
                CoordinatorSdkErrorVariant::NotFound,
            )),
        ]);

        let unavailable = reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;
        assert!(matches!(
            unavailable.outcome,
            CoordinatorReconciliationOutcome::NoWrite(
                CoordinatorReconciliationNoWriteOutcome::TemporalUnavailable { .. }
            )
        ));
        let not_found = reconcile_current_execution(
            &adapter,
            &fixture.reader,
            &fixture.projector,
            repair_request(&activation),
        )
        .await;
        assert!(matches!(
            not_found.outcome,
            CoordinatorReconciliationOutcome::NoWrite(
                CoordinatorReconciliationNoWriteOutcome::TemporalNotFound { .. }
            )
        ));
        assert!(fixture
            .reader
            .find_active_workflow_for_issue(&activation.issue_ref().repo_id, activation.issue_ref())
            .unwrap()
            .is_none());
    }
}
