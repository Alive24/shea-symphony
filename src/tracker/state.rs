use crate::config::{AssigneeFilter, RuntimeConfig};
use crate::model::{normalize_state, TrackerIssue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimDecision {
    AlreadyInProgress,
    Claimable,
    StopAndReplan { current_state: String },
}

pub fn claim_decision(issue: &TrackerIssue, config: &RuntimeConfig) -> ClaimDecision {
    let state = tracker_state_key(&issue.state);
    let state_map = &config.tracker.state_map;

    if state == tracker_state_key(&state_map.in_progress) {
        ClaimDecision::AlreadyInProgress
    } else if state == tracker_state_key(&state_map.todo)
        || state == tracker_state_key(&state_map.rework)
    {
        ClaimDecision::Claimable
    } else {
        ClaimDecision::StopAndReplan {
            current_state: issue.state.clone(),
        }
    }
}

pub(in crate::tracker) fn status_update_required(issue: &TrackerIssue, target_state: &str) -> bool {
    tracker_state_key(&issue.state) != tracker_state_key(target_state)
}

pub(in crate::tracker) fn status_is_mapped(status: &str, config: &RuntimeConfig) -> bool {
    mapped_status_names(config)
        .iter()
        .any(|mapped| tracker_state_key(mapped) == tracker_state_key(status))
}

pub(in crate::tracker) fn issue_matches_assignee_filter(
    issue: &TrackerIssue,
    filter: &AssigneeFilter,
) -> bool {
    if issue.assignees.is_empty() {
        return filter.allow_unassigned;
    }

    if filter.assignees.is_empty() {
        return true;
    }

    let allowed: Vec<String> = filter
        .assignees
        .iter()
        .map(|assignee| normalize_state(assignee))
        .collect();

    issue
        .assignees
        .iter()
        .any(|assignee| allowed.contains(&normalize_state(assignee)))
}

pub(in crate::tracker) fn tracker_state_key(state: &str) -> String {
    normalize_state(state)
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn mapped_status_names(config: &RuntimeConfig) -> Vec<&str> {
    let state_map = &config.tracker.state_map;
    vec![
        state_map.backlog.as_str(),
        state_map.todo.as_str(),
        state_map.need_to_clarify.as_str(),
        state_map.in_progress.as_str(),
        state_map.need_human_input.as_str(),
        state_map.agent_review.as_str(),
        state_map.human_review.as_str(),
        state_map.rework.as_str(),
        state_map.merging.as_str(),
        state_map.done.as_str(),
    ]
}
