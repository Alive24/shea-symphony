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
pub struct SymphonyTemporalClient {
    config: TemporalConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedIssueWorkflow {
    pub workflow_id: String,
    pub run_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum TemporalRuntimeError {
    #[error("invalid Temporal configuration: {0}")]
    InvalidConfig(String),
    #[error(
        "Temporal service is unavailable at {address} for namespace {namespace}: {source_error}"
    )]
    Unavailable {
        address: String,
        namespace: String,
        source_error: String,
    },
    #[error("failed to register Temporal worker for {task_queue}: {source_error}")]
    WorkerRegistration {
        task_queue: String,
        source_error: String,
    },
    #[error("Temporal workflow operation failed for {workflow_id}: {source_error}")]
    WorkflowOperation {
        workflow_id: String,
        source_error: String,
    },
}

impl SymphonyTemporalClient {
    pub fn new(config: TemporalConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &TemporalConfig {
        &self.config
    }

    pub async fn connect(&self) -> Result<Client, TemporalRuntimeError> {
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
        let result = client.connect().await;

        assert!(result.is_err());
        let error = result.err().unwrap();
        assert!(matches!(error, TemporalRuntimeError::Unavailable { .. }));
    }
}
