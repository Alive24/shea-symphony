//! Typed client boundary for local Temporal operations.
//!
//! This module centralizes endpoint and namespace handling so the App and
//! operator tools do not create competing Temporal clients. Starting a Workflow
//! is a durable side effect; queries are read-only; waiting for a result observes
//! an existing execution and does not advance it by itself.

use std::str::FromStr;

use temporalio_client::{
    errors::{ClientConnectError, WorkflowInteractionError, WorkflowStartError},
    Client, ClientOptions, Connection, ConnectionOptions, RetryOptions, WorkflowDescribeOptions,
    WorkflowExecutionInfo, WorkflowGetResultOptions, WorkflowHandle, WorkflowQueryOptions,
    WorkflowStartOptions,
};
use temporalio_common::protos::proto_ts_to_system_time;
use temporalio_common::protos::temporal::api::enums::v1::{
    WorkflowExecutionStatus, WorkflowIdConflictPolicy, WorkflowIdReusePolicy,
};
use temporalio_sdk_core::Url;
use thiserror::Error;
use time::{OffsetDateTime, UtcOffset};

use crate::config::TemporalConfig;
use crate::symphony::coordinator::start::{
    CoordinatorAdapterStart, CoordinatorDescribeEvidence, CoordinatorFailureKind,
    CoordinatorSdkErrorVariant, CoordinatorStartFailure, CoordinatorTemporalAdapter,
    CoordinatorTemporalPhase, CoordinatorTemporalStatus,
};
use crate::symphony::dto::{IssueWorkflowInput, IssueWorkflowQueryResult, IssueWorkflowState};
use crate::symphony::workflows::IssueWorkflow;
use crate::symphony::WorkflowId;

#[derive(Debug, Clone)]
/// High-level client for Symphony's local Temporal namespace.
///
/// The client stores configuration only. Network connections are established
/// lazily for each operation and connection failures are reported as
/// [`TemporalRuntimeError::Unavailable`].
pub struct SymphonyTemporalClient {
    config: TemporalConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Identity returned after Temporal accepts an Issue Workflow start request.
pub struct StartedIssueWorkflow {
    /// Stable application-assigned Workflow ID used for later query/result calls.
    pub workflow_id: String,
    /// Temporal Run ID when the SDK start response exposes one.
    ///
    /// A successful accepted start always returns `Some` with the real,
    /// non-empty SDK handle Run ID. The optional wire shape is retained for
    /// compatibility with existing public callers; Symphony never fabricates a
    /// fallback Run ID.
    pub run_id: Option<String>,
}

#[derive(Debug, Error)]
/// Failures produced while configuring or operating the Symphony Temporal runtime.
pub enum TemporalRuntimeError {
    /// An address or other local Temporal setting could not be parsed.
    #[error("invalid Temporal configuration: {0}")]
    InvalidConfig(String),
    /// The configured Temporal service or namespace could not be reached.
    #[error(
        "Temporal service is unavailable at {address} for namespace {namespace}: {source_error}. \
         Run `./scripts/temporal-noop-smoke` to exercise the supported local dev-service path."
    )]
    Unavailable {
        /// Configured Temporal frontend address.
        address: String,
        /// Configured Temporal namespace.
        namespace: String,
        /// SDK error text retained for operator diagnostics.
        source_error: String,
    },
    /// A worker could not register its Workflow or Activity implementations.
    #[error("failed to register Temporal worker for {task_queue}: {source_error}")]
    WorkerRegistration {
        /// Task queue whose worker registration failed.
        task_queue: String,
        /// SDK error text retained for operator diagnostics.
        source_error: String,
    },
    /// Temporal core runtime construction failed before workers could start.
    #[error("failed to initialize Temporal runtime: {0}")]
    RuntimeInitialization(String),
    /// A running worker returned a terminal runtime failure.
    #[error("Temporal worker runtime failed: {0}")]
    WorkerRuntime(String),
    /// A start, query, or result operation failed for a specific Workflow ID.
    #[error("Temporal workflow operation failed for {workflow_id}: {source_error}")]
    WorkflowOperation {
        /// Application-assigned Workflow ID used by the failed operation.
        workflow_id: String,
        /// SDK error text retained for operator diagnostics.
        source_error: String,
    },
}

impl SymphonyTemporalClient {
    /// Creates a lazy client boundary from validated runtime configuration.
    ///
    /// This performs no network I/O. Connection errors occur when an async
    /// operation is invoked.
    pub fn new(config: TemporalConfig) -> Self {
        Self { config }
    }

    /// Returns the Temporal settings used by future operations.
    pub fn config(&self) -> &TemporalConfig {
        &self.config
    }

    /// Verifies that the configured local Temporal service accepts a connection.
    ///
    /// This is a read-only readiness probe: it neither starts a Workflow nor
    /// mutates tracker, local-state, artifact, or worker state. Callers receive
    /// [`TemporalRuntimeError::Unavailable`] with the configured address and
    /// namespace when the local service cannot be reached.
    pub async fn check_service(&self) -> Result<(), TemporalRuntimeError> {
        self.connect().await.map(|_| ())
    }

    /// Connects to the configured Temporal frontend and namespace.
    ///
    /// This performs network I/O but does not start or mutate a Workflow. It is
    /// crate-visible so external consumers use the typed operations instead of
    /// bypassing Symphony's namespace and error semantics.
    pub(crate) async fn connect(&self) -> Result<Client, TemporalRuntimeError> {
        self.connect_with_retry_options(RetryOptions::default())
            .await
    }

    async fn connect_without_operation_retries(&self) -> Result<Client, TemporalRuntimeError> {
        self.connect_with_retry_options(single_attempt_retry_options())
            .await
    }

    async fn connect_with_retry_options(
        &self,
        retry_options: RetryOptions,
    ) -> Result<Client, TemporalRuntimeError> {
        let address = endpoint_url(&self.config.address)?;
        let connection_options = ConnectionOptions::new(address)
            .retry_options(retry_options)
            .build();
        let connection = Connection::connect(connection_options)
            .await
            .map_err(|error| TemporalRuntimeError::Unavailable {
                address: self.config.address.clone(),
                namespace: self.config.namespace.clone(),
                source_error: error.to_string(),
            })?;

        Client::new(
            connection,
            ClientOptions::new(self.config.namespace.as_str()).build(),
        )
        .map_err(|error| TemporalRuntimeError::Unavailable {
            address: self.config.address.clone(),
            namespace: self.config.namespace.clone(),
            source_error: error.to_string(),
        })
    }

    /// Starts the quickstart no-op Issue Workflow using the input Workflow ID.
    ///
    /// This is a durable side effect: Temporal records the Workflow start and
    /// serialized [`IssueWorkflowInput`] in history. Duplicate IDs are handled
    /// by Temporal's start semantics and surface as
    /// [`TemporalRuntimeError::WorkflowOperation`]; this method does not silently
    /// generate a replacement ID or retry with altered identity.
    ///
    /// On success, the return value confirms Temporal accepted the start request.
    /// It does not mean the Workflow or its Activities have completed.
    pub async fn start_noop_issue_workflow(
        &self,
        input: IssueWorkflowInput,
    ) -> Result<StartedIssueWorkflow, TemporalRuntimeError> {
        // Start uncertainty is recovered through stable identity, not hidden
        // transport retries inside this single caller invocation.
        let client = self.connect_without_operation_retries().await?;
        let workflow_id = input.workflow_id.clone();
        let handle = client
            .start_workflow(
                IssueWorkflow::run,
                input,
                WorkflowStartOptions::new(
                    self.config.task_queues.core.as_str(),
                    workflow_id.as_str(),
                )
                .id_reuse_policy(WorkflowIdReusePolicy::RejectDuplicate)
                .id_conflict_policy(WorkflowIdConflictPolicy::Fail)
                .build(),
            )
            .await
            .map_err(|error| TemporalRuntimeError::WorkflowOperation {
                workflow_id: workflow_id.clone(),
                source_error: error.to_string(),
            })?;
        let run_id = handle
            .run_id()
            .filter(|run_id| !run_id.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| TemporalRuntimeError::WorkflowOperation {
                workflow_id: workflow_id.clone(),
                source_error: "SDK accepted start without a non-empty Run ID".to_string(),
            })?;

        Ok(StartedIssueWorkflow {
            workflow_id,
            run_id: Some(run_id),
        })
    }

    /// Reads the current replay-derived state of an existing Issue Workflow.
    ///
    /// Temporal Query handlers are read-only and cannot perform Activities or
    /// external I/O. A successful value is an observation, not authorization for
    /// tracker progression. Missing executions and query failures are returned as
    /// [`TemporalRuntimeError::WorkflowOperation`].
    pub async fn query_issue_workflow(
        &self,
        workflow_id: &str,
    ) -> Result<IssueWorkflowQueryResult, TemporalRuntimeError> {
        let client = self.connect().await?;
        let handle = client.get_workflow_handle::<IssueWorkflow>(workflow_id);
        handle
            .query(
                IssueWorkflow::current_state,
                (),
                WorkflowQueryOptions::default(),
            )
            .await
            .map_err(|error| TemporalRuntimeError::WorkflowOperation {
                workflow_id: workflow_id.to_string(),
                source_error: error.to_string(),
            })
    }

    /// Waits for and decodes the terminal result of an Issue Workflow execution.
    ///
    /// This call may remain pending while the Workflow is open. It does not
    /// signal, update, or otherwise advance the execution. Closed-workflow and
    /// result-decoding failures are returned as
    /// [`TemporalRuntimeError::WorkflowOperation`].
    pub async fn get_issue_workflow_result(
        &self,
        workflow_id: &str,
    ) -> Result<IssueWorkflowState, TemporalRuntimeError> {
        let client = self.connect().await?;
        let handle = client.get_workflow_handle::<IssueWorkflow>(workflow_id);
        handle
            .get_result(WorkflowGetResultOptions::default())
            .await
            .map_err(|error| TemporalRuntimeError::WorkflowOperation {
                workflow_id: workflow_id.to_string(),
                source_error: error.to_string(),
            })
    }
}

impl CoordinatorTemporalAdapter for SymphonyTemporalClient {
    async fn start_issue_workflow(
        &self,
        input: IssueWorkflowInput,
    ) -> Result<CoordinatorAdapterStart, CoordinatorStartFailure> {
        let workflow_id = WorkflowId::new(input.workflow_id.clone());
        let client = self
            .connect_for_coordinator(&workflow_id, None, CoordinatorTemporalPhase::Connect)
            .await?;

        // Reject both closed-ID reuse and running-ID conflict. Retrying an
        // uncertain caller request must reuse this exact activation ID.
        match client
            .start_workflow(
                IssueWorkflow::run,
                input,
                WorkflowStartOptions::new(
                    self.config.task_queues.core.as_str(),
                    workflow_id.as_str(),
                )
                .id_reuse_policy(WorkflowIdReusePolicy::RejectDuplicate)
                .id_conflict_policy(WorkflowIdConflictPolicy::Fail)
                // TODO(T2607-03): Temporal Search Attributes/Visibility
                // indexing remains undesigned until #504/#505 establish
                // repair/read-model and real caller boundaries.
                .build(),
            )
            .await
        {
            Ok(handle) => Ok(CoordinatorAdapterStart::Accepted {
                run_id: handle.run_id().unwrap_or_default().to_owned(),
            }),
            Err(WorkflowStartError::AlreadyStarted { run_id, source }) => {
                Ok(typed_already_started(run_id, source.code()))
            }
            Err(WorkflowStartError::PayloadConversion(_)) => Err(CoordinatorStartFailure::new(
                CoordinatorTemporalPhase::Start,
                CoordinatorFailureKind::InputConfigurationPayload,
                workflow_id,
                None,
                CoordinatorSdkErrorVariant::PayloadConversion,
                None,
            )),
            Err(WorkflowStartError::Rpc(status)) => {
                let code = status.code();
                let failure = CoordinatorStartFailure::new(
                    CoordinatorTemporalPhase::Start,
                    if start_rpc_is_indeterminate(code) {
                        CoordinatorFailureKind::UnavailableOrIndeterminate
                    } else if code == temporalio_client::tonic::Code::AlreadyExists {
                        CoordinatorFailureKind::MalformedProtocolEvidence
                    } else {
                        CoordinatorFailureKind::DefinitiveServerRejection
                    },
                    workflow_id,
                    None,
                    CoordinatorSdkErrorVariant::Rpc,
                    Some(code),
                );
                if start_rpc_is_indeterminate(code) {
                    Ok(CoordinatorAdapterStart::Indeterminate(failure))
                } else {
                    Err(failure)
                }
            }
            Err(_) => Ok(CoordinatorAdapterStart::Indeterminate(
                CoordinatorStartFailure::new(
                    CoordinatorTemporalPhase::Start,
                    CoordinatorFailureKind::UnavailableOrIndeterminate,
                    workflow_id,
                    None,
                    CoordinatorSdkErrorVariant::Other,
                    None,
                ),
            )),
        }
    }

    async fn describe_issue_workflow(
        &self,
        workflow_id: &WorkflowId,
        run_id: Option<&str>,
    ) -> Result<CoordinatorDescribeEvidence, CoordinatorStartFailure> {
        let workflow_id = workflow_id.clone();
        let run_id = run_id.map(str::to_owned);
        let client = self
            .connect_for_coordinator(
                &workflow_id,
                run_id.clone(),
                CoordinatorTemporalPhase::Describe,
            )
            .await?;
        let handle = WorkflowHandle::<_, IssueWorkflow>::new(
            client,
            WorkflowExecutionInfo {
                namespace: self.config.namespace.clone(),
                workflow_id: workflow_id.as_str().to_owned(),
                // A known Run ID pins Describe to the accepted or duplicate
                // execution; otherwise Temporal resolves the current run.
                run_id: run_id.clone(),
                first_execution_run_id: None,
            },
        );

        let description = handle
            .describe(WorkflowDescribeOptions::default())
            .await
            .map_err(|error| describe_failure(&workflow_id, run_id.clone(), error))?;

        // SDK convenience accessors assume required protobuf fields exist.
        // Inspect raw evidence so a malformed response becomes a typed
        // Coordinator result instead of panicking the caller.
        let Some(info) = description.raw().workflow_execution_info.as_ref() else {
            return Ok(CoordinatorDescribeEvidence {
                workflow_id: String::new(),
                run_id: String::new(),
                temporal_started_at: None,
                status: None,
            });
        };
        let execution = info.execution.as_ref();
        Ok(CoordinatorDescribeEvidence {
            workflow_id: execution
                .map(|value| value.workflow_id.clone())
                .unwrap_or_default(),
            run_id: execution
                .map(|value| value.run_id.clone())
                .unwrap_or_default(),
            temporal_started_at: info
                .start_time
                .as_ref()
                .and_then(proto_ts_to_system_time)
                .map(OffsetDateTime::from)
                .map(|value| value.to_offset(UtcOffset::UTC)),
            status: WorkflowExecutionStatus::try_from(info.status)
                .ok()
                .and_then(map_temporal_status),
        })
    }
}

impl SymphonyTemporalClient {
    async fn connect_for_coordinator(
        &self,
        workflow_id: &WorkflowId,
        known_run_id: Option<String>,
        phase: CoordinatorTemporalPhase,
    ) -> Result<Client, CoordinatorStartFailure> {
        let address = endpoint_url(&self.config.address).map_err(|_| {
            CoordinatorStartFailure::new(
                phase,
                CoordinatorFailureKind::InputConfigurationPayload,
                workflow_id.clone(),
                known_run_id.clone(),
                CoordinatorSdkErrorVariant::InvalidConfiguration,
                None,
            )
        })?;
        // Coordinator owns uncertain outcomes explicitly; hidden SDK retries
        // would violate the one-start/one-Describe invocation contract.
        let connection_options = ConnectionOptions::new(address)
            .retry_options(single_attempt_retry_options())
            .build();
        let connection = Connection::connect(connection_options)
            .await
            .map_err(|error| connect_failure(workflow_id, known_run_id.clone(), phase, error))?;

        Client::new(
            connection,
            ClientOptions::new(self.config.namespace.as_str()).build(),
        )
        .map_err(|_| {
            CoordinatorStartFailure::new(
                phase,
                CoordinatorFailureKind::InputConfigurationPayload,
                workflow_id.clone(),
                known_run_id,
                CoordinatorSdkErrorVariant::ClientConstruction,
                None,
            )
        })
    }
}

fn connect_failure(
    workflow_id: &WorkflowId,
    known_run_id: Option<String>,
    phase: CoordinatorTemporalPhase,
    error: ClientConnectError,
) -> CoordinatorStartFailure {
    let (kind, grpc_code) = match &error {
        ClientConnectError::InvalidUri(_)
        | ClientConnectError::InvalidHeaders(_)
        | ClientConnectError::InvalidConfig(_) => {
            (CoordinatorFailureKind::InputConfigurationPayload, None)
        }
        ClientConnectError::SystemInfoCallError(status) => (
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            Some(status.code()),
        ),
        _ => (CoordinatorFailureKind::UnavailableOrIndeterminate, None),
    };
    CoordinatorStartFailure::new(
        phase,
        kind,
        workflow_id.clone(),
        known_run_id,
        CoordinatorSdkErrorVariant::ClientConnect,
        grpc_code,
    )
}

fn describe_failure(
    workflow_id: &WorkflowId,
    run_id: Option<String>,
    error: WorkflowInteractionError,
) -> CoordinatorStartFailure {
    let (kind, variant, grpc_code) = match error {
        WorkflowInteractionError::NotFound(status) => (
            // Not-found after an uncertain start cannot prove that no start
            // occurred; retain it as an unresolved Describe observation.
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            CoordinatorSdkErrorVariant::NotFound,
            Some(status.code()),
        ),
        WorkflowInteractionError::PayloadConversion(_) => (
            CoordinatorFailureKind::MalformedProtocolEvidence,
            CoordinatorSdkErrorVariant::PayloadConversion,
            None,
        ),
        WorkflowInteractionError::Rpc(status) => (
            if start_rpc_is_indeterminate(status.code()) {
                CoordinatorFailureKind::UnavailableOrIndeterminate
            } else {
                CoordinatorFailureKind::DefinitiveServerRejection
            },
            CoordinatorSdkErrorVariant::Rpc,
            Some(status.code()),
        ),
        WorkflowInteractionError::Other(_) => (
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            CoordinatorSdkErrorVariant::Other,
            None,
        ),
        _ => (
            CoordinatorFailureKind::UnavailableOrIndeterminate,
            CoordinatorSdkErrorVariant::Other,
            None,
        ),
    };
    CoordinatorStartFailure::new(
        CoordinatorTemporalPhase::Describe,
        kind,
        workflow_id.clone(),
        run_id,
        variant,
        grpc_code,
    )
}

fn typed_already_started(
    run_id: Option<String>,
    grpc_code: temporalio_client::tonic::Code,
) -> CoordinatorAdapterStart {
    CoordinatorAdapterStart::AlreadyStarted { run_id, grpc_code }
}

fn start_rpc_is_indeterminate(code: temporalio_client::tonic::Code) -> bool {
    use temporalio_client::tonic::Code;

    matches!(
        code,
        Code::Cancelled
            | Code::Unknown
            | Code::DeadlineExceeded
            | Code::Aborted
            | Code::Internal
            | Code::Unavailable
            | Code::DataLoss
    )
}

fn map_temporal_status(status: WorkflowExecutionStatus) -> Option<CoordinatorTemporalStatus> {
    match status {
        WorkflowExecutionStatus::Unspecified => None,
        WorkflowExecutionStatus::Running => Some(CoordinatorTemporalStatus::Running),
        WorkflowExecutionStatus::Completed => Some(CoordinatorTemporalStatus::Completed),
        WorkflowExecutionStatus::Failed => Some(CoordinatorTemporalStatus::Failed),
        WorkflowExecutionStatus::Canceled => Some(CoordinatorTemporalStatus::Canceled),
        WorkflowExecutionStatus::Terminated => Some(CoordinatorTemporalStatus::Terminated),
        WorkflowExecutionStatus::ContinuedAsNew => Some(CoordinatorTemporalStatus::ContinuedAsNew),
        WorkflowExecutionStatus::TimedOut => Some(CoordinatorTemporalStatus::TimedOut),
        WorkflowExecutionStatus::Paused => Some(CoordinatorTemporalStatus::Paused),
    }
}

fn single_attempt_retry_options() -> RetryOptions {
    RetryOptions::no_retries()
}

fn endpoint_url(address: &str) -> Result<Url, TemporalRuntimeError> {
    // Workflow config uses operator-friendly local addresses by default; the
    // SDK expects a URL, so normalize missing schemes at this boundary.
    let normalized = if address.contains("://") {
        address.to_string()
    } else {
        format!("http://{address}")
    };

    Url::from_str(&normalized)
        .map_err(|error| TemporalRuntimeError::InvalidConfig(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TemporalTaskQueuesConfig, TemporalWorkerConfig};
    use crate::symphony::task_queues::{AGENT_TASK_QUEUE, CORE_TASK_QUEUE, LOCAL_TASK_QUEUE};

    fn config(address: &str) -> TemporalConfig {
        TemporalConfig {
            address: address.to_string(),
            namespace: "default".to_string(),
            task_queues: TemporalTaskQueuesConfig {
                core: CORE_TASK_QUEUE.to_string(),
                agent: AGENT_TASK_QUEUE.to_string(),
                local: LOCAL_TASK_QUEUE.to_string(),
            },
            worker: TemporalWorkerConfig {
                core_concurrency: 3,
                agent_concurrency: 3,
                local_concurrency: 8,
            },
        }
    }

    #[test]
    fn endpoint_url_defaults_to_local_http_scheme() {
        let url = endpoint_url("localhost:7233").unwrap();

        assert_eq!(url.as_str(), "http://localhost:7233/");
    }

    #[test]
    fn coordinator_side_effect_client_disables_sdk_operation_retries() {
        let options = single_attempt_retry_options();

        assert_eq!(options.max_retries, 1);
        assert_eq!(options.max_elapsed_time, None);
    }

    #[tokio::test]
    async fn missing_local_temporal_maps_to_unavailable_error() {
        let client = SymphonyTemporalClient::new(config("127.0.0.1:1"));
        let result = client.check_service().await;

        assert!(result.is_err());
        let error = result.err().unwrap();
        assert!(matches!(error, TemporalRuntimeError::Unavailable { .. }));
        assert!(error.to_string().contains("temporal-noop-smoke"));
    }

    #[tokio::test]
    async fn coordinator_describe_connect_failure_retains_phase_identity_and_known_run() {
        let client = SymphonyTemporalClient::new(config("http://["));
        let workflow_id = WorkflowId::new("issue:502");

        let result = client
            .connect_for_coordinator(
                &workflow_id,
                Some("run-known".to_string()),
                CoordinatorTemporalPhase::Describe,
            )
            .await;
        let Err(failure) = result else {
            panic!("invalid endpoint unexpectedly connected");
        };

        assert_eq!(failure.phase(), CoordinatorTemporalPhase::Describe);
        assert_eq!(
            failure.kind(),
            CoordinatorFailureKind::InputConfigurationPayload
        );
        assert_eq!(failure.workflow_id(), &workflow_id);
        assert_eq!(failure.known_run_id(), Some("run-known"));
        assert_eq!(
            failure.sdk_error_variant(),
            CoordinatorSdkErrorVariant::InvalidConfiguration
        );
        assert_eq!(failure.grpc_code(), None);
    }

    #[test]
    fn typed_duplicate_mapping_preserves_run_id_and_grpc_code() {
        assert_eq!(
            typed_already_started(
                Some("run-existing".to_string()),
                temporalio_client::tonic::Code::AlreadyExists,
            ),
            CoordinatorAdapterStart::AlreadyStarted {
                run_id: Some("run-existing".to_string()),
                grpc_code: temporalio_client::tonic::Code::AlreadyExists,
            }
        );
    }
}
