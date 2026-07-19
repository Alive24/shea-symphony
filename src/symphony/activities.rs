//! Temporal Activity registrations for Symphony task queues.
//!
//! Activities are the only Symphony layer allowed to perform external side
//! effects such as tracker mutation, agent execution, filesystem access, and
//! local-state projection. Workflow code schedules these functions and records
//! their results; it must not perform the side effects itself.

use std::time::Duration;

use temporalio_macros::activities;
use temporalio_sdk::activities::{ActivityContext, ActivityError};

use crate::symphony::dto::{NoopActivityRequest, NoopActivityResult};

const TEMPORAL_SMOKE_ISSUE_PREFIX: &str = "synthetic:temporal-smoke:";
const TEMPORAL_SMOKE_ENABLED_ENV: &str = "SHEA_TEMPORAL_SMOKE";
const TEMPORAL_SMOKE_QUERY_HOLD_ENV: &str = "SHEA_TEMPORAL_SMOKE_QUERY_HOLD_MS";
const MAX_TEMPORAL_SMOKE_QUERY_HOLD_MS: u64 = 5_000;

/// Durable Activity type names polled from the latency-sensitive core queue.
///
/// Temporal records these strings in Workflow history. Renaming one without a
/// compatibility worker can make existing histories unable to replay or resume.
pub(super) const CORE_ACTIVITY_TYPES: &[&str] = &["NoopCoreActivity", "TrackerTransitionActivity"];

/// Durable Activity type names polled from the resource-heavy agent queue.
///
/// These names are persisted in Temporal history and therefore require an
/// explicit compatibility plan before they are changed or removed.
pub(super) const AGENT_ACTIVITY_TYPES: &[&str] = &[
    "MainAgentActivity",
    "ReworkActivity",
    "AgentReviewActivity",
    "MergeActivity",
];
/// Durable Activity type names polled from the short-running local queue.
///
/// The local queue may update rebuildable projections, but its Activities do
/// not become workflow or tracker authority.
pub(super) const LOCAL_ACTIVITY_TYPES: &[&str] = &[
    "LocalStateProjectionActivity",
    "ArtifactIndexActivity",
    "LocalHealthActivity",
];

#[derive(Debug, Default, Clone)]
/// Activity implementation set registered on the core task queue.
///
/// This type is `pub` because `#[activities]` generates a public SDK
/// registration interface. The containing module remains private, so it is not
/// part of Symphony's external API.
pub struct CoreActivities;

#[activities]
impl CoreActivities {
    #[activity(name = "NoopCoreActivity")]
    /// Proves core Activity routing without performing any external side effect.
    ///
    /// The returned success describes the no-op execution; it does not imply a
    /// tracker transition, local-state write, or agent action occurred.
    pub async fn noop_core_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        if let Some(delay) = smoke_query_hold(&request) {
            // Only the test-owned worker sets both explicit smoke variables,
            // and they are ignored unless the synthetic smoke issue prefix is
            // present.
            // The delay creates a deterministic read-only Query window without
            // adding a timer, clock read, or test branch to Workflow code.
            tokio::time::sleep(delay).await;
        }

        // The skeleton needs one successful Activity to prove worker routing
        // without touching tracker, SQLite, worktrees, artifacts, or agents.
        Ok(NoopActivityResult::success(&request))
    }

    #[activity(name = "TrackerTransitionActivity")]
    #[allow(dead_code)] // `#[activities]` exposes registration metadata indirectly.
    /// Placeholder for the durable, idempotent tracker-transition Activity.
    ///
    /// This skeleton intentionally returns `not_implemented` and performs no
    /// tracker mutation. A later implementation must use readback verification
    /// and retry-safe mutation identifiers.
    pub async fn tracker_transition_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }
}

fn smoke_query_hold(request: &NoopActivityRequest) -> Option<Duration> {
    let smoke_enabled = std::env::var(TEMPORAL_SMOKE_ENABLED_ENV).ok();
    let configured_delay = std::env::var(TEMPORAL_SMOKE_QUERY_HOLD_ENV).ok();
    smoke_query_hold_from_values(
        &request.issue_ref,
        smoke_enabled.as_deref(),
        configured_delay.as_deref(),
    )
}

fn smoke_query_hold_from_values(
    issue_ref: &str,
    smoke_enabled: Option<&str>,
    value: Option<&str>,
) -> Option<Duration> {
    if !matches!(smoke_enabled, Some("1") | Some("true") | Some("yes"))
        || !issue_ref.starts_with(TEMPORAL_SMOKE_ISSUE_PREFIX)
    {
        return None;
    }

    let milliseconds = value?.parse::<u64>().ok()?;
    (milliseconds > 0 && milliseconds <= MAX_TEMPORAL_SMOKE_QUERY_HOLD_MS)
        .then(|| Duration::from_millis(milliseconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_query_hold_requires_test_owned_input_and_a_bounded_value() {
        assert_eq!(
            smoke_query_hold_from_values("synthetic:temporal-smoke:issue", Some("1"), Some("2500")),
            Some(Duration::from_millis(2500))
        );
        assert_eq!(
            smoke_query_hold_from_values("#489", Some("1"), Some("2500")),
            None,
            "normal product input must never inherit the smoke-only delay"
        );
        assert_eq!(
            smoke_query_hold_from_values("synthetic:temporal-smoke:issue", Some("0"), Some("2500")),
            None,
            "the normal worker never enables the test-only hold"
        );
        assert_eq!(
            smoke_query_hold_from_values("synthetic:temporal-smoke:issue", Some("1"), Some("5001")),
            None
        );
        assert_eq!(
            smoke_query_hold_from_values(
                "synthetic:temporal-smoke:issue",
                Some("1"),
                Some("invalid")
            ),
            None
        );
    }
}

#[derive(Debug, Default, Clone)]
/// Activity implementation set registered on the long-running agent task queue.
///
/// Public visibility is required by the Temporal activity macro; the private
/// containing module keeps this generated registration type internal.
pub struct AgentActivities;

#[activities]
impl AgentActivities {
    #[activity(name = "MainAgentActivity")]
    #[allow(dead_code)] // `#[activities]` exposes registration metadata indirectly.
    /// Placeholder for a coarse Main Agent execution Activity.
    pub async fn main_agent_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }

    #[activity(name = "ReworkActivity")]
    #[allow(dead_code)] // `#[activities]` exposes registration metadata indirectly.
    /// Placeholder for a coarse Main-lane rework execution Activity.
    pub async fn rework_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }

    #[activity(name = "AgentReviewActivity")]
    #[allow(dead_code)] // `#[activities]` exposes registration metadata indirectly.
    /// Placeholder for an independent Agent Review execution Activity.
    pub async fn agent_review_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }

    #[activity(name = "MergeActivity")]
    #[allow(dead_code)] // `#[activities]` exposes registration metadata indirectly.
    /// Placeholder for the merge/land Activity and its verified terminal writes.
    pub async fn merge_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }
}

#[derive(Debug, Default, Clone)]
/// Activity implementation set registered on the short-running local task queue.
///
/// Public visibility is required by the Temporal activity macro; the private
/// containing module keeps this generated registration type internal.
pub struct LocalActivities;

#[activities]
impl LocalActivities {
    #[activity(name = "LocalStateProjectionActivity")]
    #[allow(dead_code)] // `#[activities]` exposes registration metadata indirectly.
    /// Placeholder for projecting authoritative facts into the local read model.
    pub async fn local_state_projection_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }

    #[activity(name = "ArtifactIndexActivity")]
    #[allow(dead_code)] // `#[activities]` exposes registration metadata indirectly.
    /// Placeholder for indexing artifact metadata without copying artifact bodies.
    pub async fn artifact_index_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::not_implemented(&request))
    }

    #[activity(name = "LocalHealthActivity")]
    #[allow(dead_code)] // `#[activities]` exposes registration metadata indirectly.
    /// Proves local queue routing without writing SQLite or other local state.
    pub async fn local_health_activity(
        _ctx: ActivityContext,
        request: NoopActivityRequest,
    ) -> Result<NoopActivityResult, ActivityError> {
        Ok(NoopActivityResult::success(&request))
    }
}
