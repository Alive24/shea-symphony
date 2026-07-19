//! Typed client boundary for local Temporal operations.
//!
//! This module centralizes endpoint and namespace handling so the App and
//! operator tools do not create competing Temporal clients. Starting a Workflow
//! is a durable side effect; queries are read-only; waiting for a result observes
//! an existing execution and does not advance it by itself.

use std::str::FromStr;

use temporalio_client::{
    Client, ClientOptions, Connection, ConnectionOptions, WorkflowGetResultOptions,
    WorkflowQueryOptions, WorkflowStartOptions,
};
use temporalio_sdk_core::Url;
use thiserror::Error;

use crate::config::TemporalConfig;
use crate::symphony::dto::{IssueWorkflowInput, IssueWorkflowQueryResult, IssueWorkflowState};
use crate::symphony::workflows::IssueWorkflow;

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
    /// The 2607 skeleton currently returns `None`; callers must use
    /// [`workflow_id`](Self::workflow_id) as the durable lookup identity rather
    /// than inventing a local Run ID.
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
        let address = endpoint_url(&self.config.address)?;
        let connection_options = ConnectionOptions::new(address).build();
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
        let client = self.connect().await?;
        let workflow_id = input.workflow_id.clone();
        client
            .start_workflow(
                IssueWorkflow::run,
                input,
                WorkflowStartOptions::new(
                    self.config.task_queues.core.as_str(),
                    workflow_id.as_str(),
                )
                .build(),
            )
            .await
            .map_err(|error| TemporalRuntimeError::WorkflowOperation {
                workflow_id: workflow_id.clone(),
                source_error: error.to_string(),
            })?;

        Ok(StartedIssueWorkflow {
            workflow_id,
            run_id: None,
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

    #[tokio::test]
    async fn missing_local_temporal_maps_to_unavailable_error() {
        let client = SymphonyTemporalClient::new(config("127.0.0.1:1"));
        let result = client.check_service().await;

        assert!(result.is_err());
        let error = result.err().unwrap();
        assert!(matches!(error, TemporalRuntimeError::Unavailable { .. }));
        assert!(error.to_string().contains("temporal-noop-smoke"));
    }
}
