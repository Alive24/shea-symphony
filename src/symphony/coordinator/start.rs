//! Temporal-authoritative start and immediate execution observation.
//!
//! This crate-private boundary consumes only validated executable activation
//! facts. One invocation issues at most one start request followed by at most
//! one Describe request, preserving uncertain side effects and partial
//! evidence without consulting tracker or SQLite state.

use temporalio_client::tonic::Code;
use time::OffsetDateTime;

use crate::symphony::dto::IssueWorkflowInput;

use super::CoordinatorExecutableActivation;
use crate::symphony::WorkflowId;

/// Phase in which a Coordinator Temporal failure was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoordinatorTemporalPhase {
    /// Temporal client configuration or connection establishment.
    Connect,
    /// The single Workflow start request.
    Start,
    /// The single immediate current-execution Describe request.
    Describe,
}

/// Bounded semantic category for a Temporal boundary failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoordinatorFailureKind {
    /// Input, configuration, or payload conversion failed before start dispatch.
    InputConfigurationPayload,
    /// Temporal definitively rejected the requested operation.
    DefinitiveServerRejection,
    /// Availability or transport evidence cannot determine the side effect.
    UnavailableOrIndeterminate,
    /// Temporal or adapter evidence was missing or internally contradictory.
    MalformedProtocolEvidence,
}

/// SDK error variant retained without unbounded SDK payload text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoordinatorSdkErrorVariant {
    /// Symphony rejected invalid Temporal configuration.
    InvalidConfiguration,
    /// The SDK failed while connecting to Temporal.
    ClientConnect,
    /// The SDK failed while constructing the namespace client.
    ClientConstruction,
    /// The SDK could not serialize or decode a payload.
    PayloadConversion,
    /// The SDK returned typed duplicate-start evidence.
    AlreadyStarted,
    /// The SDK returned a gRPC operation failure.
    Rpc,
    /// The SDK reported that the described execution was not found.
    NotFound,
    /// The SDK returned a future or otherwise unclassified error variant.
    Other,
    /// Shea rejected malformed evidence after a nominal SDK success.
    EvidenceValidation,
}

/// Bounded failure evidence for one Temporal phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoordinatorStartFailure {
    phase: CoordinatorTemporalPhase,
    kind: CoordinatorFailureKind,
    workflow_id: WorkflowId,
    known_run_id: Option<String>,
    sdk_error_variant: CoordinatorSdkErrorVariant,
    grpc_code: Option<Code>,
}

impl CoordinatorStartFailure {
    /// Builds bounded phase evidence without retaining SDK messages or payloads.
    pub(crate) fn new(
        phase: CoordinatorTemporalPhase,
        kind: CoordinatorFailureKind,
        workflow_id: WorkflowId,
        known_run_id: Option<String>,
        sdk_error_variant: CoordinatorSdkErrorVariant,
        grpc_code: Option<Code>,
    ) -> Self {
        Self {
            phase,
            kind,
            workflow_id,
            known_run_id,
            sdk_error_variant,
            grpc_code,
        }
    }

    /// Returns the phase that produced the evidence.
    pub(crate) const fn phase(&self) -> CoordinatorTemporalPhase {
        self.phase
    }

    /// Returns the bounded semantic failure category.
    pub(crate) const fn kind(&self) -> CoordinatorFailureKind {
        self.kind
    }

    /// Borrows the exact retry-stable Workflow ID.
    pub(crate) const fn workflow_id(&self) -> &WorkflowId {
        &self.workflow_id
    }

    /// Borrows a Run ID known at the failing boundary, when any.
    pub(crate) fn known_run_id(&self) -> Option<&str> {
        self.known_run_id.as_deref()
    }

    /// Returns the typed SDK error variant.
    pub(crate) const fn sdk_error_variant(&self) -> CoordinatorSdkErrorVariant {
        self.sdk_error_variant
    }

    /// Returns the gRPC status code when the SDK exposed one.
    pub(crate) const fn grpc_code(&self) -> Option<Code> {
        self.grpc_code
    }
}

/// Evidence produced by exactly one Temporal start attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoordinatorStartEvidence {
    /// Temporal accepted a new execution and returned its real Run ID.
    Accepted {
        /// Temporal-native Run ID from the SDK start handle.
        run_id: String,
    },
    /// Temporal rejected the exact ID because an execution already exists.
    AlreadyStarted {
        /// Existing Run ID when Temporal included it in typed error details.
        run_id: Option<String>,
    },
    /// Transport or availability evidence cannot determine the start side effect.
    Indeterminate,
}

impl CoordinatorStartEvidence {
    fn known_run_id(&self) -> Option<&str> {
        match self {
            Self::Accepted { run_id } => Some(run_id),
            Self::AlreadyStarted { run_id } => run_id.as_deref(),
            Self::Indeterminate => None,
        }
    }
}

/// Temporal execution status retained from current Describe evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoordinatorTemporalStatus {
    /// Execution is accepting Workflow Tasks.
    Running,
    /// Execution completed successfully.
    Completed,
    /// Execution failed.
    Failed,
    /// Execution was canceled.
    Canceled,
    /// Execution was terminated.
    Terminated,
    /// Execution continued under a new Run ID.
    ContinuedAsNew,
    /// Execution timed out.
    TimedOut,
    /// Execution is paused but remains open.
    Paused,
}

impl CoordinatorTemporalStatus {
    /// Returns the stable lowercase snake-case Temporal spelling.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Terminated => "terminated",
            Self::ContinuedAsNew => "continued_as_new",
            Self::TimedOut => "timed_out",
            Self::Paused => "paused",
        }
    }

    const fn is_open(self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }
}

/// Current Describe observation, independent from start evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoordinatorExecutionObservation {
    /// Current execution remains open.
    Open {
        /// Exact Temporal Workflow ID.
        workflow_id: WorkflowId,
        /// Exact Temporal Run ID.
        run_id: String,
        /// Temporal server's authoritative execution start time.
        temporal_started_at: OffsetDateTime,
        /// Current open execution status.
        status: CoordinatorTemporalStatus,
    },
    /// Current execution closed before or during immediate observation.
    Closed {
        /// Exact Temporal Workflow ID.
        workflow_id: WorkflowId,
        /// Exact Temporal Run ID.
        run_id: String,
        /// Temporal server's authoritative execution start time.
        temporal_started_at: OffsetDateTime,
        /// Current terminal execution status.
        status: CoordinatorTemporalStatus,
    },
    /// One Describe was insufficient to establish a valid current observation.
    DescribeRequired,
}

/// Combined start and Describe facts from one bounded Coordinator invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoordinatorStartResult {
    /// Exact retry-stable Workflow ID consumed by this invocation.
    pub(crate) workflow_id: WorkflowId,
    /// Evidence from the single start request.
    pub(crate) start_evidence: CoordinatorStartEvidence,
    /// Independent observation from the single immediate Describe request.
    pub(crate) execution_observation: CoordinatorExecutionObservation,
    /// Start uncertainty diagnostic when [`CoordinatorStartEvidence::Indeterminate`].
    pub(crate) start_failure: Option<CoordinatorStartFailure>,
    /// gRPC status retained from typed duplicate-start SDK evidence.
    pub(crate) already_started_grpc_code: Option<Code>,
    /// SDK variant retained when duplicate evidence was observed.
    pub(crate) already_started_sdk_error_variant: Option<CoordinatorSdkErrorVariant>,
    /// Describe diagnostic when observation remains required.
    pub(crate) describe_failure: Option<CoordinatorStartFailure>,
}

/// Raw successful Describe fields returned by a Temporal adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoordinatorDescribeEvidence {
    /// Workflow ID reported by Temporal.
    pub(crate) workflow_id: String,
    /// Run ID reported by Temporal.
    pub(crate) run_id: String,
    /// Server execution start time, absent only in malformed protocol evidence.
    pub(crate) temporal_started_at: Option<OffsetDateTime>,
    /// Raw typed status; `None` represents the protocol's unspecified value.
    pub(crate) status: Option<CoordinatorTemporalStatus>,
}

/// Result of one adapter start request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoordinatorAdapterStart {
    /// Temporal accepted the request.
    Accepted {
        /// Run ID retained from the returned SDK handle.
        run_id: String,
    },
    /// The SDK preserved typed duplicate-start evidence.
    AlreadyStarted {
        /// Existing Run ID when supplied by Temporal error details.
        run_id: Option<String>,
        /// Original gRPC status code retained from the SDK error.
        grpc_code: Code,
    },
    /// Start may have occurred despite the observed operation failure.
    Indeterminate(CoordinatorStartFailure),
}

/// Minimal Temporal operations owned by the Coordinator start boundary.
pub(crate) trait CoordinatorTemporalAdapter {
    /// Issues exactly one start request using the supplied durable input.
    async fn start_issue_workflow(
        &self,
        input: IssueWorkflowInput,
    ) -> Result<CoordinatorAdapterStart, CoordinatorStartFailure>;

    /// Issues exactly one Describe by Workflow ID and known Run ID when present.
    async fn describe_issue_workflow(
        &self,
        workflow_id: &WorkflowId,
        run_id: Option<&str>,
    ) -> Result<CoordinatorDescribeEvidence, CoordinatorStartFailure>;
}

/// Starts and immediately describes one validated executable activation.
///
/// A caller retry must pass the same activation again. This function never
/// generates a replacement ID, retries a side effect, polls, scans, or touches
/// tracker/SQLite state.
pub(crate) async fn start_executable_activation<A: CoordinatorTemporalAdapter>(
    adapter: &A,
    activation: CoordinatorExecutableActivation,
) -> Result<CoordinatorStartResult, CoordinatorStartFailure> {
    let workflow_id = activation.workflow_id().clone();
    let input = issue_workflow_input(&activation);

    // One SDK call owns the start side effect. An indeterminate transport
    // result is retained as evidence and never converted into "not started".
    let adapter_start = adapter.start_issue_workflow(input).await?;
    let normalized_start = normalize_start(&workflow_id, adapter_start)?;
    let start_evidence = normalized_start.evidence;
    let known_run_id = start_evidence.known_run_id().map(str::to_owned);

    // Describe is a separate observation. A no-op execution may already be
    // closed, so start acceptance never implies the Open variant.
    let described = adapter
        .describe_issue_workflow(&workflow_id, known_run_id.as_deref())
        .await;
    let (execution_observation, describe_failure) = match described {
        Ok(evidence) => match normalize_describe(&workflow_id, known_run_id.as_deref(), evidence) {
            Ok(observation) => (observation, None),
            Err(failure) => (
                CoordinatorExecutionObservation::DescribeRequired,
                Some(failure),
            ),
        },
        Err(failure) => (
            CoordinatorExecutionObservation::DescribeRequired,
            Some(failure),
        ),
    };

    Ok(CoordinatorStartResult {
        workflow_id,
        start_evidence,
        execution_observation,
        start_failure: normalized_start.failure,
        already_started_grpc_code: normalized_start.already_started_grpc_code,
        already_started_sdk_error_variant: normalized_start.already_started_sdk_error_variant,
        describe_failure,
    })
}

fn issue_workflow_input(activation: &CoordinatorExecutableActivation) -> IssueWorkflowInput {
    let issue_ref = activation.issue_ref();
    let source_ref = activation.source_ref().to_owned();
    IssueWorkflowInput {
        workflow_id: activation.workflow_id().as_str().to_owned(),
        repo_id: issue_ref.repo_id.database_key(),
        tracker_backend: issue_ref.tracker_backend.as_str().to_owned(),
        issue_ref: issue_ref.display_ref(),
        from_tracker_state: activation.observed().state().as_str().to_owned(),
        target_kind: activation.target_kind().as_str().to_owned(),
        source_kind: activation.source_kind().as_str().to_owned(),
        source_ref: source_ref.clone(),
        source_tracker_revision: activation.observed().revision().to_owned(),
        // This timestamp identifies the pre-start activation episode. It must
        // remain distinct from Temporal's Describe-backed execution start time.
        started_at: format_rfc3339_utc_seconds(activation.episode_time()),
        audit_reason: activation.audit_reason().to_owned(),
        operator_action_ref: (activation.source_kind()
            == super::CoordinatorSourceKind::OperatorAction)
            .then_some(source_ref),
        // Capacity admission is deliberately unowned by this slice.
        capacity_policy_ref: None,
    }
}

fn format_rfc3339_utc_seconds(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

struct NormalizedStart {
    evidence: CoordinatorStartEvidence,
    failure: Option<CoordinatorStartFailure>,
    already_started_grpc_code: Option<Code>,
    already_started_sdk_error_variant: Option<CoordinatorSdkErrorVariant>,
}

fn normalize_start(
    workflow_id: &WorkflowId,
    start: CoordinatorAdapterStart,
) -> Result<NormalizedStart, CoordinatorStartFailure> {
    match start {
        CoordinatorAdapterStart::Accepted { run_id } if !run_id.is_empty() => Ok(NormalizedStart {
            evidence: CoordinatorStartEvidence::Accepted { run_id },
            failure: None,
            already_started_grpc_code: None,
            already_started_sdk_error_variant: None,
        }),
        CoordinatorAdapterStart::Accepted { run_id } => Err(malformed_failure(
            CoordinatorTemporalPhase::Start,
            workflow_id,
            nonempty_run_id(run_id),
        )),
        CoordinatorAdapterStart::AlreadyStarted { run_id, grpc_code }
            if run_id.as_deref() != Some("") && grpc_code == Code::AlreadyExists =>
        {
            Ok(NormalizedStart {
                evidence: CoordinatorStartEvidence::AlreadyStarted { run_id },
                failure: None,
                already_started_grpc_code: Some(grpc_code),
                already_started_sdk_error_variant: Some(CoordinatorSdkErrorVariant::AlreadyStarted),
            })
        }
        CoordinatorAdapterStart::AlreadyStarted { run_id, grpc_code } => {
            Err(CoordinatorStartFailure::new(
                CoordinatorTemporalPhase::Start,
                CoordinatorFailureKind::MalformedProtocolEvidence,
                workflow_id.clone(),
                run_id.filter(|value| !value.is_empty()),
                CoordinatorSdkErrorVariant::EvidenceValidation,
                Some(grpc_code),
            ))
        }
        CoordinatorAdapterStart::Indeterminate(failure) => Ok(NormalizedStart {
            evidence: CoordinatorStartEvidence::Indeterminate,
            failure: Some(failure),
            already_started_grpc_code: None,
            already_started_sdk_error_variant: None,
        }),
    }
}

fn normalize_describe(
    expected_workflow_id: &WorkflowId,
    expected_run_id: Option<&str>,
    described: CoordinatorDescribeEvidence,
) -> Result<CoordinatorExecutionObservation, CoordinatorStartFailure> {
    let valid_identity = described.workflow_id == expected_workflow_id.as_str()
        && !described.run_id.is_empty()
        && expected_run_id.is_none_or(|run_id| run_id == described.run_id);
    let known_run_id = expected_run_id
        .map(str::to_owned)
        .or_else(|| nonempty_run_id(described.run_id.clone()));
    let Some(temporal_started_at) = described.temporal_started_at else {
        return Err(malformed_failure(
            CoordinatorTemporalPhase::Describe,
            expected_workflow_id,
            known_run_id,
        ));
    };
    let Some(status) = described.status else {
        return Err(malformed_failure(
            CoordinatorTemporalPhase::Describe,
            expected_workflow_id,
            known_run_id,
        ));
    };
    if !valid_identity {
        return Err(malformed_failure(
            CoordinatorTemporalPhase::Describe,
            expected_workflow_id,
            known_run_id,
        ));
    }

    let fields = (
        expected_workflow_id.clone(),
        described.run_id,
        temporal_started_at,
        status,
    );
    if status.is_open() {
        Ok(CoordinatorExecutionObservation::Open {
            workflow_id: fields.0,
            run_id: fields.1,
            temporal_started_at: fields.2,
            status: fields.3,
        })
    } else {
        Ok(CoordinatorExecutionObservation::Closed {
            workflow_id: fields.0,
            run_id: fields.1,
            temporal_started_at: fields.2,
            status: fields.3,
        })
    }
}

fn nonempty_run_id(run_id: String) -> Option<String> {
    (!run_id.is_empty()).then_some(run_id)
}

fn malformed_failure(
    phase: CoordinatorTemporalPhase,
    workflow_id: &WorkflowId,
    known_run_id: Option<String>,
) -> CoordinatorStartFailure {
    CoordinatorStartFailure::new(
        phase,
        CoordinatorFailureKind::MalformedProtocolEvidence,
        workflow_id.clone(),
        known_run_id,
        CoordinatorSdkErrorVariant::EvidenceValidation,
        None,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use time::macros::datetime;

    use super::*;
    use crate::symphony::coordinator::{
        CoordinatorActivationDecision, CoordinatorActivationRequest, CoordinatorSourceKind,
        CoordinatorTrackerState, ObservedTrackerSnapshot,
    };
    use crate::symphony::{IssueRef, RepoId, TrackerBackend};

    #[derive(Debug)]
    struct FakeAdapter {
        start: Mutex<Option<Result<CoordinatorAdapterStart, CoordinatorStartFailure>>>,
        describe: Mutex<Option<Result<CoordinatorDescribeEvidence, CoordinatorStartFailure>>>,
        calls: Mutex<Vec<(&'static str, String, Option<String>)>>,
        input: Mutex<Option<IssueWorkflowInput>>,
    }

    impl FakeAdapter {
        fn new(
            start: Result<CoordinatorAdapterStart, CoordinatorStartFailure>,
            describe: Result<CoordinatorDescribeEvidence, CoordinatorStartFailure>,
        ) -> Self {
            Self {
                start: Mutex::new(Some(start)),
                describe: Mutex::new(Some(describe)),
                calls: Mutex::new(Vec::new()),
                input: Mutex::new(None),
            }
        }
    }

    impl CoordinatorTemporalAdapter for FakeAdapter {
        async fn start_issue_workflow(
            &self,
            input: IssueWorkflowInput,
        ) -> Result<CoordinatorAdapterStart, CoordinatorStartFailure> {
            let result = self.start.lock().unwrap().take().unwrap();
            self.calls
                .lock()
                .unwrap()
                .push(("start", input.workflow_id.clone(), None));
            *self.input.lock().unwrap() = Some(input);
            result
        }

        async fn describe_issue_workflow(
            &self,
            workflow_id: &WorkflowId,
            run_id: Option<&str>,
        ) -> Result<CoordinatorDescribeEvidence, CoordinatorStartFailure> {
            let result = self.describe.lock().unwrap().take().unwrap();
            self.calls.lock().unwrap().push((
                "describe",
                workflow_id.as_str().to_owned(),
                run_id.map(str::to_owned),
            ));
            result
        }
    }

    fn activation(source_kind: CoordinatorSourceKind) -> CoordinatorExecutableActivation {
        let request = CoordinatorActivationRequest::new(
            IssueRef::new(
                TrackerBackend::GithubProjectV2,
                RepoId::new("github.com", "Alive24", "shea-symphony"),
                502,
            ),
            Some(CoordinatorTrackerState::Todo),
            Some("revision-502".to_owned()),
            datetime!(2026-07-28 09:44:00 UTC),
            source_kind,
            "operator/action 502",
            "Start the validated activation.",
        )
        .unwrap();
        let observed =
            ObservedTrackerSnapshot::new(CoordinatorTrackerState::Todo, "revision-502").unwrap();
        let CoordinatorActivationDecision::Executable(activation) =
            request.evaluate(&observed).unwrap()
        else {
            panic!("expected executable activation");
        };
        activation
    }

    fn described(
        activation: &CoordinatorExecutableActivation,
        run_id: &str,
        status: CoordinatorTemporalStatus,
    ) -> CoordinatorDescribeEvidence {
        CoordinatorDescribeEvidence {
            workflow_id: activation.workflow_id().as_str().to_owned(),
            run_id: run_id.to_owned(),
            temporal_started_at: Some(datetime!(2026-07-28 09:44:01 UTC)),
            status: Some(status),
        }
    }

    fn failure(
        activation: &CoordinatorExecutableActivation,
        phase: CoordinatorTemporalPhase,
        kind: CoordinatorFailureKind,
        variant: CoordinatorSdkErrorVariant,
        code: Option<Code>,
    ) -> CoordinatorStartFailure {
        CoordinatorStartFailure::new(
            phase,
            kind,
            activation.workflow_id().clone(),
            None,
            variant,
            code,
        )
    }

    #[tokio::test]
    async fn activation_mapping_populates_every_durable_input_field() {
        let activation = activation(CoordinatorSourceKind::OperatorAction);
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::Accepted {
                run_id: "run-accepted".to_owned(),
            }),
            Ok(described(
                &activation,
                "run-accepted",
                CoordinatorTemporalStatus::Running,
            )),
        );

        let result = start_executable_activation(&adapter, activation.clone())
            .await
            .unwrap();
        let input = adapter.input.lock().unwrap().clone().unwrap();

        assert_eq!(input.workflow_id, activation.workflow_id().as_str());
        assert_eq!(input.repo_id, "github.com/Alive24/shea-symphony");
        assert_eq!(input.tracker_backend, "github_project_v2");
        assert_eq!(input.issue_ref, "#502");
        assert_eq!(input.from_tracker_state, "todo");
        assert_eq!(input.target_kind, "work");
        assert_eq!(input.source_kind, "operator-action");
        assert_eq!(input.source_ref, "operator/action 502");
        assert_eq!(input.source_tracker_revision, "revision-502");
        assert_eq!(input.started_at, "2026-07-28T09:44:00Z");
        assert_eq!(input.audit_reason, "Start the validated activation.");
        assert_eq!(
            input.operator_action_ref.as_deref(),
            Some("operator/action 502")
        );
        assert_eq!(input.capacity_policy_ref, None);
        assert!(matches!(
            result.start_evidence,
            CoordinatorStartEvidence::Accepted { ref run_id } if run_id == "run-accepted"
        ));
    }

    #[test]
    fn non_operator_activation_cannot_gain_operator_or_capacity_provenance() {
        let activation = activation(CoordinatorSourceKind::Doctor);
        let input = issue_workflow_input(&activation);

        assert_eq!(input.source_kind, "doctor");
        assert_eq!(input.source_ref, activation.source_ref());
        assert_eq!(input.operator_action_ref, None);
        assert_eq!(input.capacity_policy_ref, None);
        assert_eq!(input.workflow_id, activation.workflow_id().as_str());
        assert_eq!(input.target_kind, activation.target_kind().as_str());
        assert_eq!(input.started_at, "2026-07-28T09:44:00Z");
    }

    #[tokio::test]
    async fn accepted_start_and_closed_describe_remain_orthogonal() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::Accepted {
                run_id: "run-fast".to_owned(),
            }),
            Ok(described(
                &activation,
                "run-fast",
                CoordinatorTemporalStatus::Completed,
            )),
        );

        let result = start_executable_activation(&adapter, activation)
            .await
            .unwrap();

        assert!(matches!(
            result.start_evidence,
            CoordinatorStartEvidence::Accepted { ref run_id } if run_id == "run-fast"
        ));
        assert!(matches!(
            result.execution_observation,
            CoordinatorExecutionObservation::Closed {
                ref run_id,
                status: CoordinatorTemporalStatus::Completed,
                ..
            } if run_id == "run-fast"
        ));
    }

    #[tokio::test]
    async fn duplicate_without_run_id_describes_current_execution_by_workflow_id() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::AlreadyStarted {
                run_id: None,
                grpc_code: Code::AlreadyExists,
            }),
            Ok(described(
                &activation,
                "run-existing",
                CoordinatorTemporalStatus::Running,
            )),
        );

        let result = start_executable_activation(&adapter, activation)
            .await
            .unwrap();

        assert_eq!(adapter.calls.lock().unwrap()[1].2, None);
        assert!(matches!(
            result.start_evidence,
            CoordinatorStartEvidence::AlreadyStarted { run_id: None }
        ));
        assert_eq!(result.already_started_grpc_code, Some(Code::AlreadyExists));
        assert_eq!(
            result.already_started_sdk_error_variant,
            Some(CoordinatorSdkErrorVariant::AlreadyStarted)
        );
        assert!(matches!(
            result.execution_observation,
            CoordinatorExecutionObservation::Open { ref run_id, .. }
                if run_id == "run-existing"
        ));
    }

    #[tokio::test]
    async fn indeterminate_start_can_converge_to_open_describe() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let uncertainty = failure(
            &activation,
            CoordinatorTemporalPhase::Start,
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            CoordinatorSdkErrorVariant::Rpc,
            Some(Code::Unavailable),
        );
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::Indeterminate(uncertainty.clone())),
            Ok(described(
                &activation,
                "run-converged",
                CoordinatorTemporalStatus::Running,
            )),
        );

        let result = start_executable_activation(&adapter, activation)
            .await
            .unwrap();

        assert_eq!(result.start_failure, Some(uncertainty));
        assert!(matches!(
            result.start_evidence,
            CoordinatorStartEvidence::Indeterminate
        ));
        assert!(matches!(
            result.execution_observation,
            CoordinatorExecutionObservation::Open { ref run_id, .. }
                if run_id == "run-converged"
        ));
    }

    #[tokio::test]
    async fn indeterminate_start_can_converge_to_closed_describe() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let uncertainty = failure(
            &activation,
            CoordinatorTemporalPhase::Start,
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            CoordinatorSdkErrorVariant::Rpc,
            Some(Code::DeadlineExceeded),
        );
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::Indeterminate(uncertainty)),
            Ok(described(
                &activation,
                "run-converged-closed",
                CoordinatorTemporalStatus::Failed,
            )),
        );

        let result = start_executable_activation(&adapter, activation)
            .await
            .unwrap();

        assert!(matches!(
            result.start_evidence,
            CoordinatorStartEvidence::Indeterminate
        ));
        assert!(matches!(
            result.execution_observation,
            CoordinatorExecutionObservation::Closed {
                ref run_id,
                status: CoordinatorTemporalStatus::Failed,
                ..
            } if run_id == "run-converged-closed"
        ));
    }

    #[tokio::test]
    async fn unavailable_describe_after_uncertain_start_preserves_both_diagnostics() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let start_failure = failure(
            &activation,
            CoordinatorTemporalPhase::Start,
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            CoordinatorSdkErrorVariant::Rpc,
            Some(Code::Unavailable),
        );
        let describe_failure = failure(
            &activation,
            CoordinatorTemporalPhase::Describe,
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            CoordinatorSdkErrorVariant::Rpc,
            Some(Code::Unavailable),
        );
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::Indeterminate(
                start_failure.clone(),
            )),
            Err(describe_failure.clone()),
        );

        let result = start_executable_activation(&adapter, activation)
            .await
            .unwrap();

        assert_eq!(result.start_failure, Some(start_failure));
        assert_eq!(result.describe_failure, Some(describe_failure));
        assert_eq!(
            result.execution_observation,
            CoordinatorExecutionObservation::DescribeRequired
        );
    }

    #[tokio::test]
    async fn describe_not_found_preserves_duplicate_start_evidence() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let describe_failure = CoordinatorStartFailure::new(
            CoordinatorTemporalPhase::Describe,
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            activation.workflow_id().clone(),
            Some("run-closed".to_owned()),
            CoordinatorSdkErrorVariant::NotFound,
            Some(Code::NotFound),
        );
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::AlreadyStarted {
                run_id: Some("run-closed".to_owned()),
                grpc_code: Code::AlreadyExists,
            }),
            Err(describe_failure.clone()),
        );

        let result = start_executable_activation(&adapter, activation)
            .await
            .unwrap();

        assert!(matches!(
            result.start_evidence,
            CoordinatorStartEvidence::AlreadyStarted {
                run_id: Some(ref run_id)
            } if run_id == "run-closed"
        ));
        assert_eq!(result.describe_failure, Some(describe_failure));
        assert_eq!(
            result.execution_observation,
            CoordinatorExecutionObservation::DescribeRequired
        );
        let failure = result.describe_failure.unwrap();
        assert_eq!(failure.phase(), CoordinatorTemporalPhase::Describe);
        assert_eq!(
            failure.kind(),
            CoordinatorFailureKind::UnavailableOrIndeterminate
        );
        assert_eq!(
            failure.sdk_error_variant(),
            CoordinatorSdkErrorVariant::NotFound
        );
        assert_eq!(failure.grpc_code(), Some(Code::NotFound));
        assert_eq!(failure.workflow_id(), &result.workflow_id);
        assert_eq!(failure.known_run_id(), Some("run-closed"));
    }

    #[tokio::test]
    async fn malformed_or_contradictory_describe_requires_new_observation() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::Accepted {
                run_id: "run-known".to_owned(),
            }),
            Ok(CoordinatorDescribeEvidence {
                workflow_id: activation.workflow_id().as_str().to_owned(),
                run_id: "run-contradiction".to_owned(),
                temporal_started_at: Some(datetime!(2026-07-28 09:44:01 UTC)),
                status: Some(CoordinatorTemporalStatus::Running),
            }),
        );

        let result = start_executable_activation(&adapter, activation)
            .await
            .unwrap();
        let failure = result.describe_failure.unwrap();

        assert_eq!(
            result.execution_observation,
            CoordinatorExecutionObservation::DescribeRequired
        );
        assert_eq!(
            failure.kind(),
            CoordinatorFailureKind::MalformedProtocolEvidence
        );
        assert_eq!(failure.phase(), CoordinatorTemporalPhase::Describe);
    }

    #[tokio::test]
    async fn duplicate_and_indeterminate_starts_can_observe_closed_execution() {
        for (start, run_id, status) in [
            (
                CoordinatorAdapterStart::AlreadyStarted {
                    run_id: Some("run-duplicate-closed".to_owned()),
                    grpc_code: Code::AlreadyExists,
                },
                "run-duplicate-closed",
                CoordinatorTemporalStatus::Failed,
            ),
            (
                CoordinatorAdapterStart::Indeterminate(CoordinatorStartFailure::new(
                    CoordinatorTemporalPhase::Start,
                    CoordinatorFailureKind::UnavailableOrIndeterminate,
                    activation(CoordinatorSourceKind::Tracker)
                        .workflow_id()
                        .clone(),
                    None,
                    CoordinatorSdkErrorVariant::Rpc,
                    Some(Code::DeadlineExceeded),
                )),
                "run-indeterminate-closed",
                CoordinatorTemporalStatus::TimedOut,
            ),
        ] {
            let activation = activation(CoordinatorSourceKind::Tracker);
            let adapter = FakeAdapter::new(Ok(start), Ok(described(&activation, run_id, status)));

            let result = start_executable_activation(&adapter, activation.clone())
                .await
                .unwrap();

            assert_eq!(result.workflow_id, *activation.workflow_id());
            assert!(matches!(
                result.execution_observation,
                CoordinatorExecutionObservation::Closed {
                    run_id: ref observed_run_id,
                    ..
                } if observed_run_id == run_id
            ));
        }
    }

    #[tokio::test]
    async fn accepted_and_indeterminate_starts_can_require_later_describe() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let uncertainty = failure(
            &activation,
            CoordinatorTemporalPhase::Start,
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            CoordinatorSdkErrorVariant::Rpc,
            Some(Code::Unavailable),
        );

        for start in [
            CoordinatorAdapterStart::Accepted {
                run_id: "run-accepted-unobserved".to_owned(),
            },
            CoordinatorAdapterStart::Indeterminate(uncertainty.clone()),
        ] {
            let describe_failure = failure(
                &activation,
                CoordinatorTemporalPhase::Describe,
                CoordinatorFailureKind::UnavailableOrIndeterminate,
                CoordinatorSdkErrorVariant::Rpc,
                Some(Code::Unavailable),
            );
            let adapter = FakeAdapter::new(Ok(start), Err(describe_failure.clone()));

            let result = start_executable_activation(&adapter, activation.clone())
                .await
                .unwrap();

            assert_eq!(
                result.execution_observation,
                CoordinatorExecutionObservation::DescribeRequired
            );
            assert_eq!(result.describe_failure, Some(describe_failure));
        }
    }

    #[tokio::test]
    async fn malformed_accepted_start_is_attributed_before_describe() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::Accepted {
                run_id: String::new(),
            }),
            Ok(described(
                &activation,
                "unused",
                CoordinatorTemporalStatus::Running,
            )),
        );

        let failure = start_executable_activation(&adapter, activation)
            .await
            .unwrap_err();

        assert_eq!(failure.phase(), CoordinatorTemporalPhase::Start);
        assert_eq!(
            failure.kind(),
            CoordinatorFailureKind::MalformedProtocolEvidence
        );
        assert_eq!(
            failure.sdk_error_variant(),
            CoordinatorSdkErrorVariant::EvidenceValidation
        );
        assert_eq!(adapter.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn contradictory_typed_duplicate_code_is_malformed_evidence() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::AlreadyStarted {
                run_id: Some("run-contradiction".to_owned()),
                grpc_code: Code::Unknown,
            }),
            Ok(described(
                &activation,
                "unused",
                CoordinatorTemporalStatus::Running,
            )),
        );

        let failure = start_executable_activation(&adapter, activation)
            .await
            .unwrap_err();

        assert_eq!(
            failure.kind(),
            CoordinatorFailureKind::MalformedProtocolEvidence
        );
        assert_eq!(
            failure.sdk_error_variant(),
            CoordinatorSdkErrorVariant::EvidenceValidation
        );
        assert_eq!(failure.grpc_code(), Some(Code::Unknown));
        assert_eq!(failure.known_run_id(), Some("run-contradiction"));
        assert_eq!(adapter.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn predispatch_and_definitive_start_failures_skip_describe() {
        for (phase, kind, variant, code) in [
            (
                CoordinatorTemporalPhase::Connect,
                CoordinatorFailureKind::InputConfigurationPayload,
                CoordinatorSdkErrorVariant::InvalidConfiguration,
                None,
            ),
            (
                CoordinatorTemporalPhase::Start,
                CoordinatorFailureKind::InputConfigurationPayload,
                CoordinatorSdkErrorVariant::PayloadConversion,
                None,
            ),
            (
                CoordinatorTemporalPhase::Start,
                CoordinatorFailureKind::DefinitiveServerRejection,
                CoordinatorSdkErrorVariant::Rpc,
                Some(Code::PermissionDenied),
            ),
        ] {
            let activation = activation(CoordinatorSourceKind::Tracker);
            let start_failure = failure(&activation, phase, kind, variant, code);
            let adapter = FakeAdapter::new(
                Err(start_failure.clone()),
                Ok(described(
                    &activation,
                    "unused",
                    CoordinatorTemporalStatus::Running,
                )),
            );

            let observed = start_executable_activation(&adapter, activation)
                .await
                .unwrap_err();

            assert_eq!(observed, start_failure);
            assert_eq!(observed.phase(), phase);
            assert_eq!(adapter.calls.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn one_invocation_never_retries_or_generates_replacement_identity() {
        let activation = activation(CoordinatorSourceKind::Tracker);
        let expected_id = activation.workflow_id().as_str().to_owned();
        let adapter = FakeAdapter::new(
            Ok(CoordinatorAdapterStart::Accepted {
                run_id: "run-once".to_owned(),
            }),
            Ok(described(
                &activation,
                "run-once",
                CoordinatorTemporalStatus::Paused,
            )),
        );

        let result = start_executable_activation(&adapter, activation)
            .await
            .unwrap();
        let calls = adapter.calls.lock().unwrap();

        assert_eq!(
            calls.as_slice(),
            [
                ("start", expected_id.clone(), None),
                ("describe", expected_id, Some("run-once".to_owned()))
            ]
        );
        assert!(matches!(
            result.execution_observation,
            CoordinatorExecutionObservation::Open {
                status: CoordinatorTemporalStatus::Paused,
                ..
            }
        ));
    }
}
