use shea_symphony::config::RuntimeConfig;
use shea_symphony::lane_claim::{LaneClaim, LaneClaimState};
use shea_symphony::merge_lane::{MergeLaneDecision, MergeLaneDecisionKind};
use shea_symphony::model::TrackerIssue;
use shea_symphony::tracker::TrackerAdapter;

use crate::lanes::claim::{write_lane_claim_terminal_result, WorkerLane};
use crate::orchestration::{
    add_timeline_comment_with_recovery, append_tracker_mutation_audit, close_issue_with_recovery,
    merge_completion_recovery_key, merge_decision_recovery_key, set_state_with_recovery,
    TrackerMutationAudit, TrackerMutationOutcome,
};

use super::tick::MergeTickOutputScope;

pub(super) fn record_merge_timeline_comment_with_recovery(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    decision: &MergeLaneDecision,
    workpad: &str,
    reason: &'static str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    let key = if matches!(
        decision.kind,
        MergeLaneDecisionKind::ReadyToMerge | MergeLaneDecisionKind::AlreadyMerged
    ) {
        merge_completion_recovery_key(issue, decision.pr_url.as_deref().unwrap_or("missing-pr"))
    } else {
        merge_decision_recovery_key(issue, decision)
    };
    let outcome = add_timeline_comment_with_recovery(
        adapter,
        &issue.identifier,
        Some(issue),
        workpad,
        &key,
        "timeline_comment",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "merge once",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue.identifier),
                target: decision.pr_url.clone(),
                from_state: Some(issue.state.clone()),
                to_state: decision.target_state.map(ToOwned::to_owned),
                reason,
            },
        );
    }
    Ok(outcome)
}

pub(super) fn set_merge_state_with_recovery(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    target_state: &str,
    pr_url: Option<String>,
    reason: &'static str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    let outcome = set_state_with_recovery(
        adapter,
        &issue.identifier,
        Some(issue),
        target_state,
        "state_change",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "merge once",
                mutation_type: "state_change",
                issue_ref: Some(&issue.identifier),
                target: pr_url,
                from_state: Some(issue.state.clone()),
                to_state: Some(target_state.into()),
                reason,
            },
        );
    }
    Ok(outcome)
}

pub(crate) fn record_done_merge_lane_completion(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    merge_claim: &LaneClaim,
    workpad: &str,
    output_scope: MergeTickOutputScope,
) -> Result<(), Box<dyn std::error::Error>> {
    let pr_url = issue
        .linked_pull_requests
        .first()
        .and_then(|pr| pr.url.clone());
    let completion_decision = MergeLaneDecision {
        kind: MergeLaneDecisionKind::ReadyToMerge,
        issue_ref: issue.identifier.clone(),
        pr_url: pr_url.clone(),
        target_state: Some("done"),
        reason: "merge completed".into(),
    };
    record_merge_timeline_comment_with_recovery(
        config,
        adapter,
        issue,
        &completion_decision,
        workpad,
        "merge completion evidence",
    )?;
    write_lane_claim_terminal_result(
        config,
        adapter,
        issue,
        WorkerLane::Merging,
        merge_claim,
        LaneClaimState::Done,
        "merged",
    )?;
    set_merge_state_with_recovery(config, adapter, issue, "done", pr_url, "merge completed")?;
    close_completed_issue(
        config,
        adapter,
        &issue.identifier,
        Some(issue),
        output_scope,
    )?;
    Ok(())
}

pub(super) fn close_completed_issue(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
    output_scope: MergeTickOutputScope,
) -> Result<(), Box<dyn std::error::Error>> {
    let outcome = close_issue_with_recovery(adapter, issue_ref, initial_issue)?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "merge once",
                mutation_type: "issue_close",
                issue_ref: Some(issue_ref),
                target: None,
                from_state: initial_issue.map(|issue| issue.state.clone()),
                to_state: Some("closed".into()),
                reason: "merge completed issue closure",
            },
        );
    }
    println!(
        "{}=closed_issue issue={} outcome={}",
        output_scope.action_prefix(),
        issue_ref,
        outcome.as_str()
    );
    Ok(())
}
