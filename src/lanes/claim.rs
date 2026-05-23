use jade_symphony::config::RuntimeConfig;
use jade_symphony::lane_claim::{
    LaneClaim, LaneClaimActor, LaneClaimLane, LaneClaimSource, LaneClaimState,
};
use jade_symphony::model::{native_subissue_gate_blocker, normalize_state, TrackerIssue};
use jade_symphony::prompt::{render_prompt, PromptError};
use jade_symphony::tracker::{ProjectFieldAssignment, TrackerAdapter};

use crate::{
    append_tracker_mutation_audit, current_time_ms, set_project_field_with_recovery,
    TrackerMutationAudit,
};

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

pub(crate) fn lane_claim_for_issue(
    issue: &TrackerIssue,
    lane: LaneClaimLane,
    actor: LaneClaimActor,
    source: LaneClaimSource,
    existing: Option<&str>,
) -> LaneClaim {
    existing
        .and_then(|value| LaneClaim::parse(value).ok())
        .filter(|claim| {
            claim.lane == lane
                && claim.issue == issue.identifier
                && claim.state == LaneClaimState::Active
        })
        .unwrap_or_else(|| {
            LaneClaim::active(&issue.identifier, lane, actor, source, current_time_ms())
        })
}

pub(crate) fn render_parseable_lane_claim(
    claim: &LaneClaim,
) -> Result<String, Box<dyn std::error::Error>> {
    let value = claim.render();
    let parsed = LaneClaim::parse(&value)
        .map_err(|error| format!("rendered lane claim is not parseable: {error}; value={value}"))?;
    if parsed != *claim {
        return Err(format!(
            "rendered lane claim did not round-trip; rendered={value} parsed={parsed:?} original={claim:?}"
        )
        .into());
    }
    Ok(value)
}

pub(crate) fn render_prompt_with_claim(
    template: &str,
    issue: &TrackerIssue,
    attempt: Option<u32>,
    claim: Option<&LaneClaim>,
) -> Result<String, PromptError> {
    let mut prompt = render_prompt(template, issue, attempt)?;
    if let Some(claim) = claim {
        prompt.push_str("\n\n## Assigned Lane Claim\n\n");
        prompt.push_str("- Preserve this `run=` value in handoff evidence and summaries.\n");
        prompt.push_str(&format!("- Run: `{}`\n", claim.run));
        prompt.push_str(&format!("- Claim: `{}`\n", claim.render()));
        prompt.push_str(&format!("- Registry pointer: `{}`\n", claim.registry));
    }
    Ok(prompt)
}

pub(crate) fn write_lane_claim_field(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    lane: WorkerLane,
    claim: &LaneClaim,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let claim_value = render_parseable_lane_claim(claim)?;
    if !write {
        println!(
            "{}_pool_dry_run action=claim_field issue={} field={:?} value={:?}",
            lane.label(),
            issue.identifier,
            lane.claim_field(),
            claim_value
        );
        return Ok(());
    }
    let outcome = set_project_field_with_recovery(
        adapter,
        issue,
        &ProjectFieldAssignment {
            name: lane.claim_field().into(),
            value: claim_value.clone(),
        },
        "claim_field",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: lane.label(),
                mutation_type: "claim_field",
                issue_ref: Some(&issue.identifier),
                target: Some(format!("{}={claim_value}", lane.claim_field())),
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "lane worker claim",
            },
        );
    }
    println!(
        "{}_pool_action=claim_field issue={} field={:?} run={} outcome={}",
        lane.label(),
        issue.identifier,
        lane.claim_field(),
        claim.run,
        outcome.as_str()
    );
    Ok(())
}

pub(crate) fn write_lane_claim_state(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    lane: WorkerLane,
    claim: &LaneClaim,
    state: LaneClaimState,
) -> Result<(), Box<dyn std::error::Error>> {
    let updated = claim.with_state(state);
    let value = render_parseable_lane_claim(&updated)?;
    let outcome = set_project_field_with_recovery(
        adapter,
        issue,
        &ProjectFieldAssignment {
            name: lane.claim_field().into(),
            value: value.clone(),
        },
        "claim_field",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: lane.label(),
                mutation_type: "claim_field",
                issue_ref: Some(&issue.identifier),
                target: Some(format!("{}={value}", lane.claim_field())),
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "lane worker claim state update",
            },
        );
    }
    println!(
        "{}_pool_action=claim_field_state issue={} field={:?} run={} state={} outcome={}",
        lane.label(),
        issue.identifier,
        lane.claim_field(),
        claim.run,
        state.as_str(),
        outcome.as_str()
    );
    Ok(())
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
