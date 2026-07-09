use temporalio_sdk::WorkerOptions;

use crate::config::TemporalConfig;
use crate::symphony::activities::{
    AgentActivities, CoreActivities, LocalActivities, AGENT_ACTIVITY_TYPES, CORE_ACTIVITY_TYPES,
    LOCAL_ACTIVITY_TYPES,
};
use crate::symphony::client::TemporalRuntimeError;
use crate::symphony::workflows::{IssueWorkflow, ISSUE_WORKFLOW_TYPE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskQueueRegistration {
    pub task_queue: String,
    pub workflows: Vec<String>,
    pub activities: Vec<String>,
    pub max_concurrent_activities: usize,
}

pub fn task_queue_registrations(config: &TemporalConfig) -> Vec<TaskQueueRegistration> {
    vec![
        TaskQueueRegistration {
            task_queue: config.task_queues.core.clone(),
            workflows: vec![ISSUE_WORKFLOW_TYPE.to_string()],
            activities: CORE_ACTIVITY_TYPES
                .iter()
                .map(|activity| (*activity).to_string())
                .collect(),
            max_concurrent_activities: config.worker.core_concurrency,
        },
        TaskQueueRegistration {
            task_queue: config.task_queues.agent.clone(),
            workflows: Vec::new(),
            activities: AGENT_ACTIVITY_TYPES
                .iter()
                .map(|activity| (*activity).to_string())
                .collect(),
            max_concurrent_activities: config.worker.agent_concurrency,
        },
        TaskQueueRegistration {
            task_queue: config.task_queues.local.clone(),
            workflows: Vec::new(),
            activities: LOCAL_ACTIVITY_TYPES
                .iter()
                .map(|activity| (*activity).to_string())
                .collect(),
            max_concurrent_activities: config.worker.local_concurrency,
        },
    ]
}

pub fn core_worker_options(config: &TemporalConfig) -> Result<WorkerOptions, TemporalRuntimeError> {
    let worker_options = WorkerOptions::new(config.task_queues.core.as_str())
        .register_activities(CoreActivities)
        .register_workflow::<IssueWorkflow>()
        .map_err(|error| TemporalRuntimeError::WorkerRegistration {
            task_queue: config.task_queues.core.clone(),
            source_error: error.to_string(),
        })?
        .build();

    Ok(worker_options)
}

pub fn agent_worker_options(config: &TemporalConfig) -> WorkerOptions {
    WorkerOptions::new(config.task_queues.agent.as_str())
        .register_activities(AgentActivities)
        .build()
}

pub fn local_worker_options(config: &TemporalConfig) -> WorkerOptions {
    WorkerOptions::new(config.task_queues.local.as_str())
        .register_activities(LocalActivities)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TemporalTaskQueuesConfig, TemporalWorkerConfig};
    use crate::symphony::task_queues::{AGENT_TASK_QUEUE, CORE_TASK_QUEUE, LOCAL_TASK_QUEUE};

    fn config() -> TemporalConfig {
        TemporalConfig {
            address: "localhost:7233".to_string(),
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
    fn registrations_cover_all_starting_queues() {
        let registrations = task_queue_registrations(&config());

        assert_eq!(registrations.len(), 3);
        assert_eq!(registrations[0].task_queue, CORE_TASK_QUEUE);
        assert_eq!(registrations[1].task_queue, AGENT_TASK_QUEUE);
        assert_eq!(registrations[2].task_queue, LOCAL_TASK_QUEUE);
    }

    #[test]
    fn core_registration_owns_issue_workflow_and_core_placeholders() {
        let registrations = task_queue_registrations(&config());
        let core = &registrations[0];

        assert_eq!(core.workflows, vec![ISSUE_WORKFLOW_TYPE]);
        assert_eq!(
            core.activities,
            vec!["NoopCoreActivity", "TrackerTransitionActivity"]
        );
        assert_eq!(core.max_concurrent_activities, 3);
    }

    #[test]
    fn agent_and_local_registrations_are_activity_only() {
        let registrations = task_queue_registrations(&config());

        assert!(registrations[1].workflows.is_empty());
        assert_eq!(
            registrations[1].activities,
            vec![
                "MainAgentActivity",
                "ReworkActivity",
                "AgentReviewActivity",
                "MergeActivity"
            ]
        );
        assert!(registrations[2].workflows.is_empty());
        assert_eq!(
            registrations[2].activities,
            vec![
                "LocalStateProjectionActivity",
                "ArtifactIndexActivity",
                "LocalHealthActivity"
            ]
        );
        assert_eq!(registrations[2].max_concurrent_activities, 8);
    }
}
