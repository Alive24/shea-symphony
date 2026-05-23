use std::path::{Path, PathBuf};

use jade_symphony::config::RuntimeConfig;
use jade_symphony::doctor::ProjectAuditReport;
use jade_symphony::git_handoff::ProcessHandoffCommandRunner;
use jade_symphony::handoff::expected_merge_base_branch_for_issue;
use jade_symphony::merge_lane::{
    expected_merge_base_branch, merge_lane_decision, MergeLaneDecisionKind,
};
use jade_symphony::model::{normalize_state, TrackerIssue};
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::review::{review_run_eligibility, review_worker_key, ReviewRunEligibility};
use jade_symphony::runtime_state::load_runtime_states;
use jade_symphony::tracker::{adapter_from_config, TrackerAdapter};
use jade_symphony::workflow::WorkflowDefinition;
use serde::Serialize;

use crate::commands::gate::{evaluate_issue_for_current_source, gate_target_state};
use crate::commands::project::render_state_summary;
use crate::lanes::claim::{
    pool_claim_eligibility, select_pool_worker_issues, worker_identity, WorkerLane,
};
use crate::lanes::merge::merge_preflight_status;
use crate::lanes::review::{review_backend_kind, select_review_worker_issues};
use crate::{
    all_mapped_tracker_states, run_loop_handoff_plan, session_status_snapshots, single_line,
    warn_if_temporary_workflow_path,
};

mod readiness;

pub(crate) use readiness::{
    autopilot_doctor_report, autopilot_readiness, AutopilotCanonicalCheckout,
    AutopilotDoctorSummary, AutopilotReadiness, AutopilotRuntimeSummary,
};

pub(crate) fn autopilot_plan(
    workflow_path: PathBuf,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = build_autopilot_plan(&workflow_path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        println!("{}", render_autopilot_plan_human(&snapshot));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotPlanSnapshot {
    pub(crate) schema_version: u8,
    pub(crate) read_only: bool,
    pub(crate) workflow_path: String,
    pub(crate) state_summary: String,
    pub(crate) lanes: Vec<AutopilotLanePlan>,
    pub(crate) parked_queues: Vec<AutopilotParkedQueue>,
    pub(crate) readiness: AutopilotReadiness,
    pub(crate) doctor: AutopilotDoctorSummary,
    pub(crate) canonical_checkout: AutopilotCanonicalCheckout,
    pub(crate) runtime: AutopilotRuntimeSummary,
    pub(crate) integration_gaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotLanePlan {
    pub(crate) lane: String,
    pub(crate) status: String,
    pub(crate) selected_issue: Option<AutopilotIssueSummary>,
    pub(crate) proposed_action: String,
    pub(crate) target_state: Option<String>,
    pub(crate) reason: String,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotIssueSummary {
    pub(crate) identifier: String,
    pub(crate) title: String,
    pub(crate) state: String,
    pub(crate) url: Option<String>,
    pub(crate) priority: Option<i64>,
    pub(crate) pull_request: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotParkedQueue {
    pub(crate) name: String,
    pub(crate) state: String,
    pub(crate) count: usize,
    pub(crate) issues: Vec<AutopilotIssueSummary>,
}

fn build_autopilot_plan(
    workflow_path: &Path,
) -> Result<AutopilotPlanSnapshot, Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(workflow_path);
    let workflow = WorkflowDefinition::load(workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, workflow_path)?;
    config.validate()?;
    let adapter = adapter_from_config(&config);
    let mut integration_gaps = adapter.integration_gaps();
    let issues = adapter.fetch_issues_by_states(&all_mapped_tracker_states(&config))?;

    let (runtime_states, runtime_load_error) = match load_runtime_states(&config) {
        Ok(states) => (states, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let (sessions, session_load_error) = match session_status_snapshots(&config) {
        Ok(sessions) => (sessions, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    if let Some(error) = &runtime_load_error {
        integration_gaps.push(format!("runtime_state_load_error: {error}"));
    }
    if let Some(error) = &session_load_error {
        integration_gaps.push(format!("tmux_session_status_unavailable: {error}"));
    }

    let canonical_checkout = AutopilotCanonicalCheckout::read_current(&config);
    let doctor = autopilot_doctor_report(
        workflow_path,
        &config,
        &issues,
        &runtime_states,
        &sessions,
        integration_gaps.clone(),
    );
    build_autopilot_plan_from_parts(AutopilotPlanInputs {
        workflow_path,
        config: &config,
        adapter: adapter.as_ref(),
        issues,
        doctor_report: doctor,
        canonical_checkout,
        runtime: AutopilotRuntimeSummary::from_parts(
            &runtime_states,
            &sessions,
            runtime_load_error,
            session_load_error,
        ),
        integration_gaps,
    })
}

pub(crate) struct AutopilotPlanInputs<'a> {
    pub(crate) workflow_path: &'a Path,
    pub(crate) config: &'a RuntimeConfig,
    pub(crate) adapter: &'a dyn TrackerAdapter,
    pub(crate) issues: Vec<TrackerIssue>,
    pub(crate) doctor_report: ProjectAuditReport,
    pub(crate) canonical_checkout: AutopilotCanonicalCheckout,
    pub(crate) runtime: AutopilotRuntimeSummary,
    pub(crate) integration_gaps: Vec<String>,
}

pub(crate) fn build_autopilot_plan_from_parts(
    inputs: AutopilotPlanInputs<'_>,
) -> Result<AutopilotPlanSnapshot, Box<dyn std::error::Error>> {
    let AutopilotPlanInputs {
        workflow_path,
        config,
        adapter,
        issues,
        doctor_report,
        canonical_checkout,
        runtime,
        integration_gaps,
    } = inputs;
    let lanes = autopilot_lane_plans(config, adapter, &issues)?;
    let parked_queues = autopilot_parked_queues(config, &issues);
    let doctor = AutopilotDoctorSummary::from_report(&doctor_report);
    let readiness = autopilot_readiness(
        &lanes,
        &doctor,
        &canonical_checkout,
        &runtime,
        &integration_gaps,
    );
    Ok(AutopilotPlanSnapshot {
        schema_version: 1,
        read_only: true,
        workflow_path: workflow_path.display().to_string(),
        state_summary: state_summary_value(&issues),
        lanes,
        parked_queues,
        readiness,
        doctor,
        canonical_checkout,
        runtime,
        integration_gaps,
    })
}

fn autopilot_lane_plans(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issues: &[TrackerIssue],
) -> Result<Vec<AutopilotLanePlan>, Box<dyn std::error::Error>> {
    Ok(vec![
        autopilot_main_lane_plan(config, issues),
        autopilot_review_lane_plan(config, issues),
        autopilot_merge_lane_plan(config, adapter, issues)?,
    ])
}

fn autopilot_main_lane_plan(config: &RuntimeConfig, issues: &[TrackerIssue]) -> AutopilotLanePlan {
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

    match evaluate_issue_for_current_source(config, issue) {
        Ok(decision) if !decision.is_dispatchable() => AutopilotLanePlan {
            lane: "main".into(),
            status: "blocked".into(),
            selected_issue: Some(AutopilotIssueSummary::from_issue(issue)),
            proposed_action: "quality_gate_route".into(),
            target_state: Some(gate_target_state(&decision).into()),
            reason: format!("quality_gate={:?}", decision.kind),
            evidence: decision
                .missing
                .iter()
                .map(|missing| format!("missing={missing}"))
                .collect(),
        },
        Ok(_) => match run_loop_handoff_plan(config, issue) {
            Ok(handoff) => {
                let action = if normalize_state(&issue.state) == "in progress" {
                    "resume_main_issue"
                } else {
                    "claim_main_issue"
                };
                AutopilotLanePlan {
                    lane: "main".into(),
                    status: "ready".into(),
                    selected_issue: Some(AutopilotIssueSummary::from_issue(issue)),
                    proposed_action: action.into(),
                    target_state: Some(config.tracker.state_map.agent_review.clone()),
                    reason: "dispatchable_issue".into(),
                    evidence: vec![
                        format!("workspace={}", handoff.workspace_path.display()),
                        format!("branch={}", handoff.branch_name),
                        "source=main loop dry-run selection".into(),
                    ],
                }
            }
            Err(error) => AutopilotLanePlan {
                lane: "main".into(),
                status: "blocked".into(),
                selected_issue: Some(AutopilotIssueSummary::from_issue(issue)),
                proposed_action: "handoff_plan_failed".into(),
                target_state: Some(config.tracker.state_map.need_human_input.clone()),
                reason: error.to_string(),
                evidence: vec!["source=run_loop_handoff_plan".into()],
            },
        },
        Err(error) => AutopilotLanePlan {
            lane: "main".into(),
            status: "blocked".into(),
            selected_issue: Some(AutopilotIssueSummary::from_issue(issue)),
            proposed_action: "quality_gate_error".into(),
            target_state: Some(config.tracker.state_map.need_human_input.clone()),
            reason: error.to_string(),
            evidence: vec!["source=evaluate_issue_for_current_source".into()],
        },
    }
}

fn autopilot_review_lane_plan(
    config: &RuntimeConfig,
    issues: &[TrackerIssue],
) -> AutopilotLanePlan {
    let agent_review_state = &config.tracker.state_map.agent_review;
    let review_issues = issues
        .iter()
        .filter(|issue| issue.normalized_state() == normalize_state(agent_review_state))
        .filter(|issue| !coordination_issue_hint(issue))
        .cloned()
        .collect::<Vec<_>>();
    if review_issues.is_empty() {
        return AutopilotLanePlan {
            lane: "review".into(),
            status: "idle".into(),
            selected_issue: None,
            proposed_action: "idle".into(),
            target_state: None,
            reason: "no_agent_review_issue".into(),
            evidence: vec!["source=review loop dry-run selection".into()],
        };
    }

    let backend_kind = review_backend_kind(config, None);
    let selected =
        select_review_worker_issues(&review_issues, agent_review_state, &backend_kind, 1);
    if let Some(issue) = selected.first() {
        let worker_key = review_worker_key(issue, &backend_kind);
        return AutopilotLanePlan {
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
        };
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
    AutopilotLanePlan {
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
    }
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

fn autopilot_parked_queues(
    config: &RuntimeConfig,
    issues: &[TrackerIssue],
) -> Vec<AutopilotParkedQueue> {
    vec![
        parked_queue_for_state(
            "Human Review",
            &config.tracker.state_map.human_review,
            issues,
        ),
        parked_queue_for_state(
            "Need Human Input",
            &config.tracker.state_map.need_human_input,
            issues,
        ),
        {
            let issues = coordination_issue_summaries(config, issues);
            AutopilotParkedQueue {
                name: "Dogfood / Coordination".into(),
                state: "non-dispatchable".into(),
                count: issues.len(),
                issues,
            }
        },
    ]
}

fn parked_queue_for_state(
    name: &str,
    state: &str,
    issues: &[TrackerIssue],
) -> AutopilotParkedQueue {
    let parked = issues
        .iter()
        .filter(|issue| issue.normalized_state() == normalize_state(state))
        .map(AutopilotIssueSummary::from_issue)
        .collect::<Vec<_>>();
    AutopilotParkedQueue {
        name: name.into(),
        state: state.into(),
        count: parked.len(),
        issues: parked,
    }
}

fn coordination_issue_summaries(
    config: &RuntimeConfig,
    issues: &[TrackerIssue],
) -> Vec<AutopilotIssueSummary> {
    let done_state = normalize_state(&config.tracker.state_map.done);
    issues
        .iter()
        .filter(|issue| issue.normalized_state() != done_state && coordination_issue_hint(issue))
        .map(AutopilotIssueSummary::from_issue)
        .collect()
}

fn autopilot_lane_candidate_issues(issues: &[TrackerIssue]) -> Vec<TrackerIssue> {
    issues
        .iter()
        .filter(|issue| !coordination_issue_hint(issue))
        .cloned()
        .collect()
}

fn coordination_issue_hint(issue: &TrackerIssue) -> bool {
    let title = issue.title.trim().to_ascii_lowercase();
    let labels = issue
        .labels
        .iter()
        .map(|label| label.to_ascii_lowercase())
        .collect::<Vec<_>>();
    title.starts_with("dogfood:")
        || title.contains("dogfood session")
        || title.contains("session coordination")
        || labels.iter().any(|label| {
            matches!(
                label.as_str(),
                "dogfood-session" | "dogfood session" | "coordination"
            )
        })
}

impl AutopilotIssueSummary {
    fn from_issue(issue: &TrackerIssue) -> Self {
        Self {
            identifier: issue.identifier.clone(),
            title: issue.title.clone(),
            state: issue.state.clone(),
            url: issue.url.clone(),
            priority: issue.priority,
            pull_request: issue.linked_pull_requests.first().and_then(|pr| {
                pr.url
                    .clone()
                    .or_else(|| pr.number.map(|n| format!("#{n}")))
            }),
        }
    }
}

fn state_summary_value(issues: &[TrackerIssue]) -> String {
    render_state_summary(issues)
        .strip_prefix("state_summary=")
        .unwrap_or("(unknown)")
        .to_string()
}

fn render_autopilot_plan_human(snapshot: &AutopilotPlanSnapshot) -> String {
    let mut lines = vec![
        "Autopilot Plan".to_string(),
        "read_only=true".to_string(),
        format!("workflow={}", snapshot.workflow_path),
        format!("state_summary={}", snapshot.state_summary),
        format!(
            "readiness={} reason={}",
            snapshot.readiness.status, snapshot.readiness.reason
        ),
        String::new(),
        "Lanes".to_string(),
    ];
    for lane in &snapshot.lanes {
        let selected = lane
            .selected_issue
            .as_ref()
            .map(|issue| format!("{} {}", issue.identifier, single_line(&issue.title)))
            .unwrap_or_else(|| "none".into());
        lines.push(format!(
            "- {} status={} selected={} action={} target={} reason={}",
            lane.lane,
            lane.status,
            selected,
            lane.proposed_action,
            lane.target_state.as_deref().unwrap_or("none"),
            lane.reason
        ));
        for evidence in lane.evidence.iter().take(3) {
            lines.push(format!("  evidence={evidence}"));
        }
    }
    lines.push(String::new());
    lines.push("Parked Queues".into());
    for queue in &snapshot.parked_queues {
        let issues = if queue.issues.is_empty() {
            "none".into()
        } else {
            queue
                .issues
                .iter()
                .map(|issue| issue.identifier.clone())
                .collect::<Vec<_>>()
                .join(",")
        };
        lines.push(format!(
            "- {} count={} issues={}",
            queue.name, queue.count, issues
        ));
    }
    lines.push(String::new());
    lines.push("Readiness Evidence".into());
    lines.push(format!(
        "- doctor blockers={} warnings={}",
        snapshot.doctor.blockers, snapshot.doctor.warnings
    ));
    lines.push(format!(
        "- canonical safe_for_write={} branch={} reason={}",
        snapshot.canonical_checkout.safe_for_write,
        snapshot
            .canonical_checkout
            .branch
            .as_deref()
            .unwrap_or("unknown"),
        snapshot
            .canonical_checkout
            .reason
            .as_deref()
            .unwrap_or("none")
    ));
    lines.push(format!(
        "- runtime active_states={} sessions={} attention={}",
        snapshot.runtime.runtime_state_count,
        snapshot.runtime.session_count,
        snapshot.runtime.session_attention_count
    ));
    for blocker in &snapshot.readiness.blockers {
        lines.push(format!("  blocker={blocker}"));
    }
    for warning in snapshot.readiness.warnings.iter().take(5) {
        lines.push(format!("  warning={warning}"));
    }
    lines.join("\n")
}
