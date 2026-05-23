use std::path::{Path, PathBuf};

use jade_symphony::canonical_checkout::{
    canonical_checkout_status_line, inspect_canonical_checkout,
};
use jade_symphony::config::RuntimeConfig;
use jade_symphony::doctor::{
    append_local_skill_install_doctor_violations, audit_project_issues_with_context,
    default_jade_symphony_skill_targets, AuditSeverity, ProjectAuditReport, ProjectDoctorContext,
};
use jade_symphony::git_handoff::ProcessHandoffCommandRunner;
use jade_symphony::handoff::expected_merge_base_branch_for_issue;
use jade_symphony::merge_lane::{
    expected_merge_base_branch, merge_lane_decision, MergeLaneDecisionKind,
};
use jade_symphony::model::{normalize_state, SessionStatusSnapshot, TrackerIssue};
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::review::{review_run_eligibility, review_worker_key, ReviewRunEligibility};
use jade_symphony::runtime_state::{load_runtime_states, RuntimeState};
use jade_symphony::skill_status::{doctor_skill_readiness_summary, SkillStatusInput};
use jade_symphony::tracker::{adapter_from_config, TrackerAdapter};
use jade_symphony::workflow::WorkflowDefinition;
use serde::Serialize;

use crate::commands::doctor::{
    append_canonical_checkout_doctor_violations, append_workspace_doctor_violations,
    discover_skill_suite_repo_root,
};
use crate::commands::gate::{evaluate_issue_for_current_source, gate_target_state};
use crate::commands::project::render_state_summary;
use crate::lanes::claim::{
    pool_claim_eligibility, select_pool_worker_issues, worker_identity, WorkerLane,
};
use crate::lanes::merge::merge_preflight_status;
use crate::lanes::review::{review_backend_kind, select_review_worker_issues};
use crate::{
    all_mapped_tracker_states, current_time_ms, run_loop_handoff_plan, session_status_snapshots,
    single_line, warn_if_temporary_workflow_path,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotReadiness {
    pub(crate) status: String,
    pub(crate) reason: String,
    pub(crate) blockers: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotDoctorSummary {
    pub(crate) blockers: usize,
    pub(crate) warnings: usize,
    pub(crate) blocker_codes: Vec<String>,
    pub(crate) warning_codes: Vec<String>,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotCanonicalCheckout {
    pub(crate) safe_for_write: bool,
    pub(crate) root: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) upstream: Option<String>,
    pub(crate) clean: Option<bool>,
    pub(crate) reason: Option<String>,
    pub(crate) status_line: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotRuntimeSummary {
    pub(crate) runtime_state_count: usize,
    pub(crate) session_count: usize,
    pub(crate) session_attention_count: usize,
    pub(crate) blockers: Vec<String>,
    pub(crate) evidence: Vec<String>,
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

fn autopilot_doctor_report(
    workflow_path: &Path,
    config: &RuntimeConfig,
    issues: &[TrackerIssue],
    runtime_states: &[RuntimeState],
    sessions: &[SessionStatusSnapshot],
    integration_gaps: Vec<String>,
) -> ProjectAuditReport {
    let context = ProjectDoctorContext {
        runtime_state: runtime_states.first().cloned(),
        runtime_states: runtime_states.to_vec(),
        sessions: sessions.to_vec(),
        now_ms: current_time_ms(),
        stale_after_ms: 10_800_000,
    };
    let mut report = audit_project_issues_with_context(issues, Some(&context));
    report.integration_gaps = integration_gaps;
    append_canonical_checkout_doctor_violations(&mut report, config);
    append_workspace_doctor_violations(&mut report, config, issues);
    if let Ok(skill_repo_root) = discover_skill_suite_repo_root(workflow_path) {
        let skill_targets = default_jade_symphony_skill_targets();
        append_local_skill_install_doctor_violations(&mut report, &skill_repo_root, &skill_targets);
        report.skill_readiness_summary = Some(doctor_skill_readiness_summary(SkillStatusInput {
            workflow_path: workflow_path.to_path_buf(),
            suite_path: None,
            codex_dir: None,
            gemini_dir: None,
            require_gemini: false,
            session_skills: Vec::new(),
            session_skills_file: None,
        }));
    }
    report
}

fn autopilot_readiness(
    lanes: &[AutopilotLanePlan],
    doctor: &AutopilotDoctorSummary,
    canonical_checkout: &AutopilotCanonicalCheckout,
    runtime: &AutopilotRuntimeSummary,
    integration_gaps: &[String],
) -> AutopilotReadiness {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if doctor.blockers > 0 {
        blockers.push(format!("doctor_blockers={}", doctor.blockers));
    }
    if !canonical_checkout.safe_for_write {
        blockers.push(format!(
            "canonical_checkout={}",
            canonical_checkout
                .reason
                .as_deref()
                .unwrap_or("not safe for future write-mode autopilot")
        ));
    }
    blockers.extend(runtime.blockers.iter().cloned());
    warnings.extend(doctor.evidence.iter().take(5).cloned());
    warnings.extend(
        integration_gaps
            .iter()
            .filter(|gap| !gap.contains("canonical_checkout"))
            .take(5)
            .cloned(),
    );

    let active_lane = lanes.iter().any(|lane| lane.status == "ready");
    let (status, reason) = if doctor.blockers > 0 || !canonical_checkout.safe_for_write {
        (
            "blocked_by_doctor_or_canonical_checkout",
            "Doctor blockers or canonical checkout safety must be resolved before write-mode autopilot.",
        )
    } else if !runtime.blockers.is_empty() {
        (
            "blocked_by_ambiguous_lane_or_runtime_state",
            "Runtime/session state needs operator attention before write-mode autopilot.",
        )
    } else if active_lane {
        (
            "ready",
            "At least one lane has dispatchable work and no readiness blocker was found.",
        )
    } else {
        (
            "idle_but_healthy",
            "All lanes are idle and no readiness blocker was found.",
        )
    };

    AutopilotReadiness {
        status: status.into(),
        reason: reason.into(),
        blockers,
        warnings,
    }
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

impl AutopilotDoctorSummary {
    fn from_report(report: &ProjectAuditReport) -> Self {
        let mut blocker_codes = Vec::new();
        let mut warning_codes = Vec::new();
        let mut evidence = Vec::new();
        for violation in &report.violations {
            match violation.severity {
                AuditSeverity::Blocker => blocker_codes.push(violation.code.clone()),
                AuditSeverity::Warning => warning_codes.push(violation.code.clone()),
            }
            evidence.push(format!(
                "{} severity={:?} code={} message={}",
                violation.issue_ref, violation.severity, violation.code, violation.message
            ));
        }
        if let Some(summary) = &report.skill_readiness_summary {
            evidence.push(summary.clone());
        }
        Self {
            blockers: report.blocker_count(),
            warnings: report
                .violations
                .iter()
                .filter(|violation| violation.severity == AuditSeverity::Warning)
                .count(),
            blocker_codes,
            warning_codes,
            evidence,
        }
    }
}

impl AutopilotCanonicalCheckout {
    fn read_current(config: &RuntimeConfig) -> Self {
        let root = match std::env::current_dir() {
            Ok(root) => root,
            Err(error) => {
                return Self::blocked(format!("current directory unavailable: {error}"));
            }
        };
        match inspect_canonical_checkout(&root, config) {
            Ok(report) => {
                let reason = canonical_checkout_readiness_blocker(&report);
                Self {
                    safe_for_write: reason.is_none(),
                    root: Some(report.root.display().to_string()),
                    branch: report.branch.clone(),
                    upstream: report.upstream.clone(),
                    clean: Some(report.is_clean()),
                    reason,
                    status_line: Some(canonical_checkout_status_line(&report)),
                }
            }
            Err(error) => Self::blocked(error.to_string()),
        }
    }

    fn blocked(reason: String) -> Self {
        Self {
            safe_for_write: false,
            root: None,
            branch: None,
            upstream: None,
            clean: None,
            reason: Some(reason),
            status_line: None,
        }
    }
}

fn canonical_checkout_readiness_blocker(
    report: &jade_symphony::canonical_checkout::CanonicalCheckoutReport,
) -> Option<String> {
    let Some(branch) = report.branch.as_deref() else {
        return Some("HEAD is detached".into());
    };
    if branch != "main" {
        return Some(format!("current branch is {branch:?}, expected \"main\""));
    }
    if let (Some(head), Some(upstream), Some(upstream_head)) = (
        report.head.as_deref(),
        report.upstream.as_deref(),
        report.upstream_head.as_deref(),
    ) {
        if head != upstream_head {
            return Some(format!(
                "local main does not match upstream {upstream} at {upstream_head}"
            ));
        }
    }
    if !report.tracked_dirty.is_empty() {
        return Some(format!(
            "tracked dirty files: {}",
            report.tracked_dirty.join(", ")
        ));
    }
    let unclassified = report
        .unclassified_untracked()
        .iter()
        .map(|entry| entry.path.display().to_string())
        .collect::<Vec<_>>();
    if !unclassified.is_empty() {
        return Some(format!(
            "unclassified untracked files: {}",
            unclassified.join(", ")
        ));
    }
    None
}

impl AutopilotRuntimeSummary {
    pub(crate) fn from_parts(
        runtime_states: &[RuntimeState],
        sessions: &[SessionStatusSnapshot],
        runtime_load_error: Option<String>,
        session_load_error: Option<String>,
    ) -> Self {
        let attention_sessions = sessions
            .iter()
            .filter(|session| session_needs_autopilot_attention(session))
            .collect::<Vec<_>>();
        let mut blockers = Vec::new();
        if let Some(error) = runtime_load_error {
            blockers.push(format!("runtime_state_load_error={error}"));
        }
        if let Some(error) = session_load_error {
            blockers.push(format!("session_status_load_error={error}"));
        }
        if !runtime_states.is_empty() {
            blockers.push(format!("active_runtime_states={}", runtime_states.len()));
        }
        if !attention_sessions.is_empty() {
            blockers.push(format!("session_attention={}", attention_sessions.len()));
        }
        let mut evidence = runtime_states
            .iter()
            .filter_map(|state| {
                state.active_issue.as_ref().map(|issue| {
                    format!(
                        "runtime issue={} lane={} backend={} session={}",
                        issue.identifier,
                        state.lane.as_deref().unwrap_or("unknown"),
                        state.backend,
                        state.backend_session_id.as_deref().unwrap_or("none")
                    )
                })
            })
            .collect::<Vec<_>>();
        evidence.extend(attention_sessions.iter().map(|session| {
            format!(
                "session={} lane={} status={} issue={}",
                session.session_id,
                session.lane,
                session.status,
                session.issue_identifier.as_deref().unwrap_or("unknown")
            )
        }));
        Self {
            runtime_state_count: runtime_states.len(),
            session_count: sessions.len(),
            session_attention_count: attention_sessions.len(),
            blockers,
            evidence,
        }
    }
}

fn session_needs_autopilot_attention(session: &SessionStatusSnapshot) -> bool {
    matches!(
        session.status.as_str(),
        "waiting_for_approval"
            | "waiting_for_human_input"
            | "waiting_for_trust"
            | "usage_limited"
            | "failed"
            | "stale"
    )
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
