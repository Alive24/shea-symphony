use jade_symphony::config::RuntimeConfig;
use jade_symphony::lane_claim::{LaneClaim, LaneClaimLane, LaneClaimState};
use jade_symphony::model::{native_subissue_gate_blocker, normalize_state, TrackerIssue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerLane {
    Main,
    Merging,
}

impl WorkerLane {
    pub(crate) fn claim_field(self) -> &'static str {
        match self {
            Self::Main => "Main Agent",
            Self::Merging => "Merging Agent",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Merging => "merging",
        }
    }

    pub(crate) fn claim_lane(self) -> LaneClaimLane {
        match self {
            Self::Main => LaneClaimLane::Main,
            Self::Merging => LaneClaimLane::Merge,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PoolClaimEligibility {
    Claimable,
    OwnedBySelf,
    ClaimedByOther { owner: String },
    WrongLaneState { state: String },
    ParentNativeSubissuesIncomplete { reason: String },
}

impl PoolClaimEligibility {
    pub(crate) fn is_claimable(&self) -> bool {
        matches!(self, Self::Claimable | Self::OwnedBySelf)
    }

    pub(crate) fn skip_reason(&self) -> String {
        match self {
            Self::Claimable | Self::OwnedBySelf => "claimable".into(),
            Self::ClaimedByOther { owner } => format!("claimed_by_other:{owner}"),
            Self::WrongLaneState { state } => format!("wrong_lane_state:{state}"),
            Self::ParentNativeSubissuesIncomplete { reason } => reason.clone(),
        }
    }
}

pub(crate) fn worker_identity(config: &RuntimeConfig, lane: WorkerLane) -> String {
    let label = config.identity.actor_label.trim();
    if label.is_empty() {
        format!("jade-symphony-{}", lane.label())
    } else {
        label.to_string()
    }
}

pub(crate) fn project_text_field(issue: &TrackerIssue, name: &str) -> Option<String> {
    issue
        .project_fields
        .get(name)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn pool_claim_eligibility(
    issue: &TrackerIssue,
    lane: WorkerLane,
    worker_id: &str,
    config: &RuntimeConfig,
) -> PoolClaimEligibility {
    let normalized_state = issue.normalized_state();
    let state_map = &config.tracker.state_map;
    let eligible_state = match lane {
        WorkerLane::Main => {
            normalized_state == normalize_state(&state_map.todo)
                || normalized_state == normalize_state(&state_map.rework)
                || normalized_state == normalize_state(&state_map.in_progress)
        }
        WorkerLane::Merging => normalized_state == normalize_state(&state_map.merging),
    };
    if !eligible_state {
        return PoolClaimEligibility::WrongLaneState {
            state: issue.state.clone(),
        };
    }
    if lane == WorkerLane::Main {
        let terminal_states = config.terminal_state_set().into_iter().collect();
        if let Some(reason) = native_subissue_gate_blocker(issue, &terminal_states) {
            return PoolClaimEligibility::ParentNativeSubissuesIncomplete { reason };
        }
    }

    match project_text_field(issue, lane.claim_field()) {
        Some(owner) if owner == worker_id => PoolClaimEligibility::OwnedBySelf,
        Some(owner) => match LaneClaim::parse(&owner) {
            Ok(claim)
                if claim.lane == lane.claim_lane() && claim.state.is_terminal_audit_pointer() =>
            {
                PoolClaimEligibility::Claimable
            }
            Ok(claim)
                if claim.lane == lane.claim_lane()
                    && claim.issue == issue.identifier
                    && claim.state == LaneClaimState::Active
                    && claim.worker.as_deref() == Some(worker_id) =>
            {
                PoolClaimEligibility::OwnedBySelf
            }
            Ok(claim) if claim.lane == lane.claim_lane() => {
                PoolClaimEligibility::ClaimedByOther { owner: claim.run }
            }
            _ => PoolClaimEligibility::ClaimedByOther { owner },
        },
        None => PoolClaimEligibility::Claimable,
    }
}

pub(crate) fn select_pool_worker_issues(
    issues: &[TrackerIssue],
    lane: WorkerLane,
    worker_id: &str,
    pool: usize,
    config: &RuntimeConfig,
) -> Vec<TrackerIssue> {
    if pool == 0 {
        return Vec::new();
    }

    let mut selected = issues
        .iter()
        .filter(|issue| pool_claim_eligibility(issue, lane, worker_id, config).is_claimable())
        .cloned()
        .collect::<Vec<_>>();
    selected.sort_by_key(|issue| issue.priority.unwrap_or(i64::MAX));
    selected.truncate(pool);
    selected
}
