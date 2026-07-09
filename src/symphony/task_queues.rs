//! Stable task-queue names for Symphony worker routing and capacity isolation.
//!
//! Queues are split by operational characteristics, not merely by source-code
//! layer. Long-running agent Activities must not delay latency-sensitive tracker
//! operations, and short local projection work must remain independently
//! scalable. Per-queue Activity concurrency is configured separately.

/// Queue for Issue Workflows and latency-sensitive control-plane Activities.
pub const CORE_TASK_QUEUE: &str = "symphony-core";
/// Queue for long-running, resource-heavy coding and review agent Activities.
pub const AGENT_TASK_QUEUE: &str = "symphony-agent";
/// Queue for short local projection, indexing, health, and rebuild Activities.
pub const LOCAL_TASK_QUEUE: &str = "symphony-local";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_task_queues_match_2607_contract() {
        assert_eq!(CORE_TASK_QUEUE, "symphony-core");
        assert_eq!(AGENT_TASK_QUEUE, "symphony-agent");
        assert_eq!(LOCAL_TASK_QUEUE, "symphony-local");
    }
}
