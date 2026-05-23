use jade_symphony::config::RuntimeConfig;
use jade_symphony::model::TrackerIssue;
use jade_symphony::tracker::{claim_decision, ClaimDecision};

use super::runtime::RuntimeRecoveryCandidate;
use crate::{live_github_tracker, select_pool_worker_issues, unbounded_loop_sleep_ms, WorkerLane};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoDispatchAction {
    Stop { reason: &'static str },
    SleepAndContinue { delay_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunLoopClaimAction {
    Claim,
    Resume,
    StopAndReplan { current_state: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssigneeOwnershipDecision {
    Allowed,
    Block { reason: String },
}

pub(crate) fn select_main_run_loop_issues(
    recoverable_runtime_states: &[RuntimeRecoveryCandidate],
    plan_selected: &[TrackerIssue],
    available_slots: usize,
    worker_id: &str,
    config: &RuntimeConfig,
) -> Vec<TrackerIssue> {
    if available_slots == 0 {
        return Vec::new();
    }

    let mut selected = Vec::new();
    for candidate in recoverable_runtime_states {
        if selected.len() >= available_slots {
            break;
        }
        let issue = candidate.issue.clone();
        println!(
            "run_loop_recovery_candidate issue={} attempt={} reason={}",
            issue.identifier, candidate.state.attempt_count, candidate.reason
        );
        if selected
            .iter()
            .any(|selected_issue: &TrackerIssue| selected_issue.identifier == issue.identifier)
        {
            continue;
        }
        selected.push(issue);
    }

    let remaining_slots = available_slots.saturating_sub(selected.len());
    let normal_selected = select_pool_worker_issues(
        plan_selected,
        WorkerLane::Main,
        worker_id,
        remaining_slots,
        config,
    );
    for issue in normal_selected {
        if !selected
            .iter()
            .any(|selected_issue: &TrackerIssue| selected_issue.identifier == issue.identifier)
        {
            selected.push(issue);
        }
    }

    selected
}

pub(crate) fn run_loop_claim_action(
    issue: &TrackerIssue,
    config: &RuntimeConfig,
) -> RunLoopClaimAction {
    match claim_decision(issue, config) {
        ClaimDecision::Claimable => RunLoopClaimAction::Claim,
        ClaimDecision::AlreadyInProgress => RunLoopClaimAction::Resume,
        ClaimDecision::StopAndReplan { current_state } => {
            RunLoopClaimAction::StopAndReplan { current_state }
        }
    }
}

pub(crate) fn run_loop_assignee_ownership_decision(
    issue: &TrackerIssue,
    config: &RuntimeConfig,
    active_login: Option<&str>,
    profile_login: Option<&str>,
) -> AssigneeOwnershipDecision {
    if !live_github_tracker(config) {
        return AssigneeOwnershipDecision::Allowed;
    }

    if issue.assignees.is_empty() {
        return if config.tracker.assignee_filter.allow_unassigned {
            AssigneeOwnershipDecision::Allowed
        } else {
            AssigneeOwnershipDecision::Block {
                reason: "live GitHub issue has no assignee".into(),
            }
        };
    }

    let identities = [profile_login, active_login]
        .into_iter()
        .flatten()
        .map(normalized_login)
        .filter(|login| !login.is_empty())
        .collect::<Vec<_>>();

    if identities.is_empty() {
        return AssigneeOwnershipDecision::Block {
            reason: "active GitHub identity unavailable for assignee ownership check".into(),
        };
    }

    let assigned = issue
        .assignees
        .iter()
        .map(|assignee| normalized_login(assignee))
        .collect::<Vec<_>>();

    if assigned
        .iter()
        .any(|assignee| identities.iter().any(|identity| identity == assignee))
    {
        AssigneeOwnershipDecision::Allowed
    } else {
        AssigneeOwnershipDecision::Block {
            reason: format!(
                "active identity {:?} does not match issue assignees {:?}",
                identities, issue.assignees
            ),
        }
    }
}

fn normalized_login(value: &str) -> String {
    value.trim().trim_start_matches('@').to_ascii_lowercase()
}

pub(crate) fn no_dispatch_action(limit: Option<usize>, poll_interval_ms: u64) -> NoDispatchAction {
    if limit.is_some() {
        return NoDispatchAction::Stop {
            reason: "no_dispatchable_issue",
        };
    }
    NoDispatchAction::SleepAndContinue {
        delay_ms: unbounded_loop_sleep_ms(limit, poll_interval_ms).unwrap_or(poll_interval_ms),
    }
}
