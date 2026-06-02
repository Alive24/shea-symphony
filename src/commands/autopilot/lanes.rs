use shea_symphony::config::RuntimeConfig;
use shea_symphony::git_handoff::ProcessHandoffCommandRunner;
use shea_symphony::handoff::expected_merge_base_branch_for_issue;
use shea_symphony::merge_lane::{
    expected_merge_base_branch, merge_lane_decision, native_linked_pull_requests_for_merge,
    MergeLaneDecisionKind,
};
use shea_symphony::model::{normalize_state, TrackerIssue};
use shea_symphony::orchestrator::Orchestrator;
use shea_symphony::review::{review_run_eligibility, review_worker_key, ReviewRunEligibility};
use shea_symphony::tracker::TrackerAdapter;

use crate::commands::gate::{evaluate_issue_for_current_source, gate_target_state};
use crate::lanes::claim::{
    pool_claim_eligibility, select_pool_worker_issues, worker_identity, WorkerLane,
};
use crate::lanes::main_loop::{run_loop_handoff_plan, run_loop_preflight_launch_workspace};
use crate::lanes::merge::merge_preflight_status;
use crate::lanes::review::{review_backend_kind, select_review_worker_issues};
use crate::orchestration::hydrate_issues_for_review_lane;

use super::{coordination_issue_hint, AutopilotIssueSummary, AutopilotLanePlan};

pub(super) fn autopilot_lane_plans(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issues: &[TrackerIssue],
) -> Result<Vec<AutopilotLanePlan>, Box<dyn std::error::Error>> {
    Ok(vec![
        autopilot_main_lane_plan(config, adapter, issues),
        autopilot_review_lane_plan(config, adapter, issues)?,
        autopilot_merge_lane_plan(config, adapter, issues)?,
    ])
}

fn autopilot_main_lane_plan(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issues: &[TrackerIssue],
) -> AutopilotLanePlan {
    let orchestrator = Orchestrator::new(config.clone());
    let lane_issues = autopilot_lane_candidate_issues(issues);
    let plan = orchestrator.plan_dispatch(lane_issues);
    let worker_id = worker_identity(config, WorkerLane::Main);
    let selected =
        select_pool_worker_issues(&plan.selected, WorkerLane::Main, &worker_id, 1, config);
    let Some(issue) = selected.first() else {
        let reason = plan
            .selected
            .first()
            .map(|candidate| {
                pool_claim_eligibility(candidate, WorkerLane::Main, &worker_id, config)
                    .skip_reason()
            })
            .unwrap_or_else(|| "no_dispatchable_issue".into());
        return AutopilotLanePlan {
            lane: "main".into(),
            status: "idle".into(),
            selected_issue: None,
            proposed_action: "idle".into(),
            target_state: None,
            reason,
            evidence: vec!["source=main loop dry-run selection".into()],
        };
    };
    let issue = match adapter.hydrate_issue_evidence(issue.clone(), issues) {
        Ok(issue) => issue,
        Err(error) => {
            return AutopilotLanePlan {
                lane: "main".into(),
                status: "blocked".into(),
                selected_issue: Some(AutopilotIssueSummary::from_issue(issue)),
                proposed_action: "quality_gate_error".into(),
                target_state: Some(config.tracker.state_map.need_human_input.clone()),
                reason: error.to_string(),
                evidence: vec!["source=hydrate_issue_evidence".into()],
            };
        }
    };

    match evaluate_issue_for_current_source(config, &issue) {
        Ok(decision) if !decision.is_dispatchable() => AutopilotLanePlan {
            lane: "main".into(),
            status: "blocked".into(),
            selected_issue: Some(AutopilotIssueSummary::from_issue(&issue)),
            proposed_action: "quality_gate_route".into(),
            target_state: Some(gate_target_state(&decision).into()),
            reason: format!("quality_gate={:?}", decision.kind),
            evidence: decision
                .missing
                .iter()
                .map(|missing| format!("missing={missing}"))
                .collect(),
        },
        Ok(_) => match run_loop_handoff_plan(config, &issue) {
            Ok(mut handoff) => {
                match run_loop_preflight_launch_workspace(config, &issue, &mut handoff) {
                    Ok(workspace_preflight) => {
                        let action = if normalize_state(&issue.state) == "in progress" {
                            "resume_main_issue"
                        } else {
                            "claim_main_issue"
                        };
                        let mut evidence = vec![
                            format!("workspace={}", handoff.workspace_path.display()),
                            format!("branch={}", handoff.branch_name),
                            "source=main loop dry-run selection".into(),
                        ];
                        evidence.extend(workspace_preflight.evidence);
                        AutopilotLanePlan {
                            lane: "main".into(),
                            status: "ready".into(),
                            selected_issue: Some(AutopilotIssueSummary::from_issue(&issue)),
                            proposed_action: action.into(),
                            target_state: Some(config.tracker.state_map.agent_review.clone()),
                            reason: "dispatchable_issue".into(),
                            evidence,
                        }
                    }
                    Err(error) => AutopilotLanePlan {
                        lane: "main".into(),
                        status: "blocked".into(),
                        selected_issue: Some(AutopilotIssueSummary::from_issue(&issue)),
                        proposed_action: "workspace_preflight_failed".into(),
                        target_state: Some(config.tracker.state_map.need_human_input.clone()),
                        reason: error.to_string(),
                        evidence: vec!["source=run_loop_preflight_launch_workspace".into()],
                    },
                }
            }
            Err(error) => AutopilotLanePlan {
                lane: "main".into(),
                status: "blocked".into(),
                selected_issue: Some(AutopilotIssueSummary::from_issue(&issue)),
                proposed_action: "handoff_plan_failed".into(),
                target_state: Some(config.tracker.state_map.need_human_input.clone()),
                reason: error.to_string(),
                evidence: vec!["source=run_loop_handoff_plan".into()],
            },
        },
        Err(error) => AutopilotLanePlan {
            lane: "main".into(),
            status: "blocked".into(),
            selected_issue: Some(AutopilotIssueSummary::from_issue(&issue)),
            proposed_action: "quality_gate_error".into(),
            target_state: Some(config.tracker.state_map.need_human_input.clone()),
            reason: error.to_string(),
            evidence: vec!["source=evaluate_issue_for_current_source".into()],
        },
    }
}

fn autopilot_review_lane_plan(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issues: &[TrackerIssue],
) -> Result<AutopilotLanePlan, Box<dyn std::error::Error>> {
    let agent_review_state = &config.tracker.state_map.agent_review;
    let review_issues = issues
        .iter()
        .filter(|issue| issue.normalized_state() == normalize_state(agent_review_state))
        .filter(|issue| !coordination_issue_hint(issue))
        .cloned()
        .collect::<Vec<_>>();
    let review_issues = hydrate_issues_for_review_lane(adapter, review_issues)?;
    if review_issues.is_empty() {
        return Ok(AutopilotLanePlan {
            lane: "review".into(),
            status: "idle".into(),
            selected_issue: None,
            proposed_action: "idle".into(),
            target_state: None,
            reason: "no_agent_review_issue".into(),
            evidence: vec!["source=review loop dry-run selection".into()],
        });
    }

    let backend_kind = review_backend_kind(config, None);
    let selected =
        select_review_worker_issues(&review_issues, agent_review_state, &backend_kind, 1);
    if let Some(issue) = selected.first() {
        let worker_key = review_worker_key(issue, &backend_kind);
        return Ok(AutopilotLanePlan {
            lane: "review".into(),
            status: "ready".into(),
            selected_issue: Some(AutopilotIssueSummary::from_issue(issue)),
            proposed_action: "start_independent_review".into(),
            target_state: Some(format!(
                "{} | {} | {} | unchanged",
                config.tracker.state_map.human_review,
                config.tracker.state_map.rework,
                config.tracker.state_map.need_human_input
            )),
            reason: "agent_review_issue".into(),
            evidence: vec![
                format!("backend={backend_kind}"),
                format!("worker_key={worker_key}"),
                "source=review loop dry-run selection".into(),
            ],
        });
    }

    let reason = review_issues
        .first()
        .map(
            |issue| match review_run_eligibility(issue, agent_review_state, &backend_kind) {
                ReviewRunEligibility::AlreadyQueued { worker_key } => {
                    format!("review_worker_exists:{worker_key}")
                }
                ReviewRunEligibility::NotInAgentReview { current_state } => {
                    format!("state_changed:{current_state}")
                }
                ReviewRunEligibility::InvalidHandoff { reason } => {
                    format!("invalid_handoff:{reason}")
                }
                ReviewRunEligibility::Eligible { .. } => "eligible".into(),
            },
        )
        .unwrap_or_else(|| "no_agent_review_issue".into());
    Ok(AutopilotLanePlan {
        lane: "review".into(),
        status: "blocked".into(),
        selected_issue: None,
        proposed_action: "skip".into(),
        target_state: None,
        reason,
        evidence: vec![
            format!("backend={backend_kind}"),
            "source=review_run_eligibility".into(),
        ],
    })
}

fn autopilot_merge_lane_plan(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issues: &[TrackerIssue],
) -> Result<AutopilotLanePlan, Box<dyn std::error::Error>> {
    let merging_state = &config.tracker.state_map.merging;
    let mut merging_issues = issues
        .iter()
        .filter(|issue| issue.normalized_state() == normalize_state(merging_state))
        .filter(|issue| !coordination_issue_hint(issue))
        .cloned()
        .collect::<Vec<_>>();
    if merging_issues.is_empty() {
        return Ok(AutopilotLanePlan {
            lane: "merge".into(),
            status: "idle".into(),
            selected_issue: None,
            proposed_action: "idle".into(),
            target_state: None,
            reason: "no_merging_issue".into(),
            evidence: vec!["source=merge loop dry-run selection".into()],
        });
    }

    merging_issues.sort_by_key(|issue| issue.priority.unwrap_or(i64::MAX));
    let worker_id = worker_identity(config, WorkerLane::Merging);
    let selected =
        select_pool_worker_issues(&merging_issues, WorkerLane::Merging, &worker_id, 1, config);
    let Some(issue) = selected.first() else {
        let reason = merging_issues
            .first()
            .map(|candidate| {
                pool_claim_eligibility(candidate, WorkerLane::Merging, &worker_id, config)
                    .skip_reason()
            })
            .unwrap_or_else(|| "no_unclaimed_merging_issue".into());
        return Ok(AutopilotLanePlan {
            lane: "merge".into(),
            status: "idle".into(),
            selected_issue: None,
            proposed_action: "idle".into(),
            target_state: None,
            reason,
            evidence: vec!["source=merge loop dry-run selection".into()],
        });
    };

    let linked_pull_requests = adapter.list_linked_pull_requests(&issue.identifier)?;
    let linked_pull_requests = native_linked_pull_requests_for_merge(config, &linked_pull_requests);
    let runner = ProcessHandoffCommandRunner;
    let expected_base =
        expected_merge_base_branch_for_issue(issue, expected_merge_base_branch(config));
    let status = merge_preflight_status(config, issue, &linked_pull_requests, &runner)?;
    let decision = merge_lane_decision(
        issue,
        merging_state,
        &expected_base,
        &linked_pull_requests,
        status.as_ref(),
    );
    let lane_status = if decision.kind.is_merge_ready()
        || decision.kind == MergeLaneDecisionKind::AlreadyMerged
    {
        "ready"
    } else if decision.target_state.is_some() {
        "blocked"
    } else {
        "waiting"
    };
    let mut evidence = vec![
        format!("decision={:?}", decision.kind),
        format!("expected_base={expected_base}"),
        "source=merge_lane_decision".into(),
    ];
    if let Some(pr_url) = decision.pr_url.as_deref() {
        evidence.push(format!("pull_request={pr_url}"));
    }
    Ok(AutopilotLanePlan {
        lane: "merge".into(),
        status: lane_status.into(),
        selected_issue: Some(AutopilotIssueSummary::from_issue(issue)),
        proposed_action: merge_action_for_decision(decision.kind).into(),
        target_state: decision.target_state.map(str::to_string),
        reason: decision.reason,
        evidence,
    })
}

fn merge_action_for_decision(kind: MergeLaneDecisionKind) -> &'static str {
    match kind {
        MergeLaneDecisionKind::ReadyToMerge => "merge_pull_request",
        MergeLaneDecisionKind::AlreadyMerged => "mark_done",
        MergeLaneDecisionKind::StaleBranch => "update_pr_branch",
        MergeLaneDecisionKind::MergeDirty => "attempt_safe_conflict_repair",
        _ => "record_merge_preflight_blocker",
    }
}

fn autopilot_lane_candidate_issues(issues: &[TrackerIssue]) -> Vec<TrackerIssue> {
    issues
        .iter()
        .filter(|issue| !coordination_issue_hint(issue))
        .cloned()
        .collect()
}
