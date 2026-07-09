use temporalio_macros::activities;
use temporalio_sdk::activities::{ActivityContext, ActivityError};

use crate::symphony::dto::{NoopActivityRequest, NoopActivityResult};

// These names are the first durable Activity-type contract. Placeholder
// implementations stay inert until later issues attach real side effects.
pub const CORE_ACTIVITY_TYPES: &[&str] = &["NoopCoreActivity", "TrackerTransitionActivity"];
pub const AGENT_ACTIVITY_TYPES: &[&str] = &[
    "MainAgentActivity",
    "ReworkActivity",
    "AgentReviewActivity",
    "MergeActivity",
];
pub const LOCAL_ACTIVITY_TYPES: &[&str] = &[
    "LocalStateProjectionActivity",
    "ArtifactIndexActivity",
    "LocalHealthActivity",
];

#[derive(Debug, Default, Clone)]
pub struct CoreActivities;

#[activities]
impl CoreActivities {
    #[activity(name = "NoopCoreActivity")]
    pub async fn noop_core_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        // The skeleton needs one successful Activity to prove worker routing
        // without touching tracker, SQLite, worktrees, artifacts, or agents.
        Ok(NoopActivityResult::success(&request))
    }

    #[activity(name = "TrackerTransitionActivity")]
    pub async fn tracker_transition_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }
}

#[derive(Debug, Default, Clone)]
pub struct AgentActivities;

#[activities]
impl AgentActivities {
    #[activity(name = "MainAgentActivity")]
    pub async fn main_agent_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }

    #[activity(name = "ReworkActivity")]
    pub async fn rework_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }

    #[activity(name = "AgentReviewActivity")]
    pub async fn agent_review_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }

    #[activity(name = "MergeActivity")]
    pub async fn merge_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }
}

#[derive(Debug, Default, Clone)]
pub struct LocalActivities;

#[activities]
impl LocalActivities {
    #[activity(name = "LocalStateProjectionActivity")]
    pub async fn local_state_projection_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }

    #[activity(name = "ArtifactIndexActivity")]
    pub async fn artifact_index_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }

    #[activity(name = "LocalHealthActivity")]
    pub async fn local_health_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::success(&request))
    }
}
