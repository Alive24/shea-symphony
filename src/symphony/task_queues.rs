use crate::config::{TemporalConfig, TemporalTaskQueuesConfig};

pub const CORE_TASK_QUEUE: &str = "symphony-core";
pub const AGENT_TASK_QUEUE: &str = "symphony-agent";
pub const LOCAL_TASK_QUEUE: &str = "symphony-local";
pub const TASK_QUEUE_COUNT: usize = 3;

pub fn default_task_queues() -> TemporalTaskQueuesConfig {
    TemporalTaskQueuesConfig {
        core: CORE_TASK_QUEUE.to_string(),
        agent: AGENT_TASK_QUEUE.to_string(),
        local: LOCAL_TASK_QUEUE.to_string(),
    }
}

pub fn configured_task_queues(config: &TemporalConfig) -> [&str; TASK_QUEUE_COUNT] {
    [
        config.task_queues.core.as_str(),
        config.task_queues.agent.as_str(),
        config.task_queues.local.as_str(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_task_queues_match_2607_contract() {
        let queues = default_task_queues();

        assert_eq!(queues.core, CORE_TASK_QUEUE);
        assert_eq!(queues.agent, AGENT_TASK_QUEUE);
        assert_eq!(queues.local, LOCAL_TASK_QUEUE);
    }
}
