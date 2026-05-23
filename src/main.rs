use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{error::ErrorKind, Args, Parser, Subcommand, ValueEnum};
use jade_symphony::agent::{
    backend_from_config, persist_prompt_artifact, usage_limit_pause_from_events, AgentBackend,
    ClaudeCodeBackend, CodexBackend, DryRunBackend, TmuxBackend, UsageLimitPause,
};
use jade_symphony::artifacts::{artifact_layout, cleanup_plan, ArtifactClass, CleanupPlan};
use jade_symphony::canonical_checkout::{
    canonical_checkout_refresh_status_line, canonical_checkout_status_line,
    canonical_checkout_warning_lines, canonical_quarantine_root, inspect_canonical_checkout,
    refresh_canonical_checkout_before_write, CanonicalCheckoutRefreshMode,
};
use jade_symphony::config::RuntimeConfig;
use jade_symphony::doctor::{
    append_local_skill_install_doctor_violations, audit_project_issues,
    audit_project_issues_with_context, default_jade_symphony_skill_targets,
    draft_pr_repair_candidates, human_review_repair_candidates, render_doctor_repair_workpad,
    render_human_review_repair_workpad, render_project_audit_report,
    render_project_audit_report_json, AuditSeverity, ProjectAuditReport, ProjectAuditViolation,
    ProjectDoctorContext, AGENT_REVIEW_DRAFT_PR,
};
use jade_symphony::event_log::{
    EventLog, EventRecord, TrackerMutationAuditInput, TrackerMutationAuditRecord,
};
use jade_symphony::git_handoff::{
    commit_issue_worktree_changes, ensure_pull_request_ready, prepare_issue_worktree,
    publish_issue_pull_request, CommandOutput, HandoffCommandRunner, LiveWorktreeResult,
    ProcessHandoffCommandRunner, PullRequestPublication, PullRequestReadyStatus,
};
use jade_symphony::handoff::{
    evaluate_agent_review_handoff, expected_merge_base_branch_for_issue,
    plan_issue_handoff_for_profile, render_agent_review_handoff_workpad,
    AgentReviewHandoffEvidence, HandoffError, IssueHandoffPlan,
};
use jade_symphony::issue_forge::{next_clarification_question, ForgeValidationReport};
use jade_symphony::issue_workspace::{
    discover_issue_workspaces, discover_issue_workspaces_from_parts, git_worktree_list,
    infer_issue_ref_from_branch_or_path, render_workspace_adoption_workpad,
    render_workspace_ensure_workpad, validate_workspace_adoption, IssueWorkspaceCandidate,
    IssueWorkspaceReport, WorkspaceMatchStrength,
};
use jade_symphony::lane_claim::{
    LaneClaim, LaneClaimActor, LaneClaimLane, LaneClaimSource, LaneClaimState,
};
use jade_symphony::merge_lane::{
    expected_merge_base_branch, fetch_pull_request_status_with_recheck, fixture_merge_output,
    merge_lane_decision, merge_lane_workpad, merge_lane_workpad_with_repair_evidence,
    merge_pull_request, pull_request_status_from_linked, repair_dirty_pull_request,
    update_pull_request_branch, MergeConflictRepairOutcome, MergeLaneDecisionKind,
    MergeRepairEvidence,
};
use jade_symphony::model::AgentEvent;
use jade_symphony::model::{
    native_subissue_gate_blocker, native_subissue_statuses, normalize_state, GateDecision,
    GateDecisionKind, LatestStatus, SessionStatusSnapshot, TrackerIssue,
};
use jade_symphony::observability_api::serve_once;
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::ownership::{
    render_runtime_ownership_marker, runtime_ownership_decision, RuntimeOwnershipDecision,
    RuntimeOwnershipMarker,
};
use jade_symphony::presentation::{
    render_doctor_panel, render_project_state_panel, render_run_loop_panel, RunLoopPanel,
};
use jade_symphony::profiles::{discover_execution_profiles, selected_execution_profile};
use jade_symphony::progress::{run_with_progress_heartbeat, ProgressHeartbeatSpec};
use jade_symphony::prompt::render_prompt;
use jade_symphony::quality_gate::{
    evaluate_issue_with_dependency_preflight, evaluate_issue_with_llm_gate,
    evaluate_issue_with_source_alignment, LlmGateMode, LlmGateOptions,
};
use jade_symphony::review::{
    classify_review_freshness, gemini_cli_headless_args, gemini_prelaunch_health_diagnostic,
    gemini_review_health_diagnostic, poll_review_job_until_terminal,
    render_repeated_review_failure_workpad, render_review_freshness_workpad, render_review_workpad,
    review_failure_signature, review_gate_decision_for_issue, review_pass_target_state,
    review_run_eligibility, review_worker_key, transition_allowed_for_main_agent,
    transition_allowed_for_review_agent, write_review_job_ledger_record, FakeReviewBackend,
    FakeReviewOutcome, GeminiCliReviewBackend, GeminiReviewRecoveryPolicy, ReviewBackend,
    ReviewFreshnessInput, ReviewGateDecision, ReviewJob, ReviewJobState, ReviewOutcome,
    ReviewRepeatedFailureEvidence, ReviewRequest, ReviewReworkClass, ReviewRunEligibility,
    ReviewStaleReason,
};
use jade_symphony::review_status::{
    load_review_status, render_project_inspect_review_summary, render_review_status_human,
    ReviewStatusOptions, DEFAULT_RECENT_REVIEW_JOBS,
};
use jade_symphony::rework::rework_transition_expected;
#[cfg(test)]
use jade_symphony::rework::{render_rework_diagnostic_workpad, ReworkDiagnostic};
use jade_symphony::runtime_state::{
    detect_runtime_stall, load_runtime_states, mark_runtime_state_updated, record_runtime_retry,
    remove_runtime_state_for_issue, runtime_state_for_issue, runtime_state_path,
    save_runtime_states, upsert_runtime_state, RuntimeIssueState, RuntimeRetryState,
    RuntimeStallState, RuntimeState, RuntimeTransition,
};
use jade_symphony::session_registry::{
    capture_tmux_pane_tail, classify_session_record, load_session_registry, read_log_tail,
    save_session_record, save_session_registry, session_registry_path, unix_timestamp_ms,
    AgentSessionRecord, SessionStatus, SessionStatusProbe,
};
use jade_symphony::skill_status::{
    build_skill_readiness_report, doctor_skill_readiness_summary, render_skill_readiness_report,
    render_skill_readiness_report_json, SkillStatusInput,
};
use jade_symphony::status_surface::{render_latest_status_bar, render_snapshot};
use jade_symphony::tracker::{
    adapter_from_config, claim_decision, classify_project_state_error,
    classify_project_state_failure_message, ClaimDecision, FollowUpIssueInput,
    ProjectFieldAssignment, ProjectStateFailureKind, TrackerAdapter, TrackerError,
};
use jade_symphony::workflow::{AgentLane, WorkflowDefinition};
use jade_symphony::workspace::{
    apply_local_git_identity, prepare_workspace, profile_scoped_identifier, remove_issue_workspace,
    run_after_run, run_before_run, run_workspace_command, safe_identifier, GitIdentityApplyResult,
    Workspace,
};
use serde::Serialize;

const DEFAULT_RUN_LOOP_BASE_BRANCH: &str = "main";
const DEFAULT_SESSION_STATUS_LINES: usize = 80;
const CODEX_APP_SERVER_HANDOFF_BOUNDARY: &str = "\n\n## Codex App-Server Runtime Boundary\n\n\
This run is executing inside the Codex app-server backend. Treat the app-server \
turn as the implementation and local-verification worker only. Do not run \
GitHub Project reads or mutations, do not create or update pull requests, and \
do not attempt final Project state transitions from inside this child turn. \
Leave a concise terminal summary of changed files, verification commands, and \
any blocker. The outer Jade Symphony CLI will commit eligible worktree changes, \
publish or update the PR, write durable workpad evidence, verify linked PR \
readback, and perform the final `Agent Review` handoff.\n";
const DEFAULT_SESSION_STALE_AFTER_MS: u64 = 15 * 60 * 1000;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = Command::parse(args)?;

    match command {
        Command::Plan {
            workflow_path,
            json,
        } => plan(workflow_path, json),
        Command::AutopilotPlan {
            workflow_path,
            json,
        } => autopilot_plan(workflow_path, json),
        Command::StatusApi {
            workflow_path,
            bind,
            once,
        } => status_api(workflow_path, bind, once),
        Command::Validate { workflow_path } => validate(workflow_path),
        Command::Inspect {
            workflow_path,
            states,
        } => inspect(workflow_path, states),
        Command::ProjectState { options } => project_state(options),
        Command::ProjectIssue {
            workflow_path,
            issue_ref,
            json,
        } => project_issue(workflow_path, issue_ref, json),
        Command::ProjectInspect {
            workflow_path,
            issue_ref,
            lane,
        } => project_inspect(workflow_path, issue_ref, lane),
        Command::Doctor { options } => doctor(options),
        Command::DoctorRepairHumanReview {
            workflow_path,
            write,
        } => doctor_repair_human_review(workflow_path, write),
        Command::SkillsStatus { input, json } => skills_status(input, json),
        Command::Profiles { workflow_path } => list_profiles(workflow_path),
        Command::Debug { workflow_path } => debug_report(workflow_path),
        Command::CleanupPlan { workflow_path } => cleanup_plan_command(workflow_path),
        Command::CleanPlan { workflow_path } => cleanup_plan_command(workflow_path),
        Command::CleanAudit { workflow_path } => clean_audit_command(workflow_path),
        Command::RunOnce { workflow_path } => run_once(workflow_path),
        Command::RunLoop { options } => run_loop(options),
        Command::CleanupWorkspaces {
            workflow_path,
            write,
        } => cleanup_workspaces(workflow_path, write),
        Command::WorkspaceList { workflow_path } => workspace_list(workflow_path),
        Command::WorkspaceShow {
            workflow_path,
            issue_ref,
        } => workspace_show(workflow_path, issue_ref),
        Command::WorkspaceAdopt {
            workflow_path,
            issue_ref,
            path,
            write,
        } => workspace_adopt(workflow_path, issue_ref, path, write),
        Command::WorkspaceEnsure {
            workflow_path,
            issue_ref,
            pr_ref,
            branch,
            write,
        } => workspace_ensure(workflow_path, issue_ref, pr_ref, branch, write),
        Command::MergeOnce {
            workflow_path,
            write,
        } => merge_once(workflow_path, write),
        Command::MergeLoop { options } => merge_loop(options),
        Command::SetState {
            workflow_path,
            issue_ref,
            state,
            write,
        } => set_state(workflow_path, issue_ref, state, write),
        Command::Workpad {
            workflow_path,
            issue_ref,
            markdown_path,
            write,
        } => upsert_workpad(workflow_path, issue_ref, markdown_path, write),
        Command::TimelineComment {
            workflow_path,
            issue_ref,
            markdown_path,
            write,
        } => append_timeline_comment(workflow_path, issue_ref, markdown_path, write),
        Command::LinkPr {
            workflow_path,
            issue_ref,
            pr_ref,
            write,
        } => link_pr(workflow_path, issue_ref, pr_ref, write),
        Command::CreateFollowUp {
            workflow_path,
            title,
            body_path,
            write,
        } => create_follow_up(workflow_path, title, body_path, write),
        Command::AddToProject {
            workflow_path,
            issue_id,
            write,
        } => add_to_project(workflow_path, issue_id, write),
        Command::ReviewFake {
            workflow_path,
            issue_ref,
            outcome,
            write,
        } => review_fake(workflow_path, issue_ref, outcome, write),
        Command::ReviewOnce {
            workflow_path,
            issue_ref,
            write,
        } => review_once(workflow_path, issue_ref, write),
        Command::ReviewClaim {
            workflow_path,
            issue_ref,
            worker,
            write,
        } => review_claim(workflow_path, issue_ref, worker, write),
        Command::LaneClaim {
            workflow_path,
            issue_ref,
            lane,
            worker,
            source,
            write,
        } => lane_claim_command(workflow_path, issue_ref, lane, worker, source, write),
        Command::ReviewClearClaim {
            workflow_path,
            issue_ref,
            write,
        } => review_clear_claim(workflow_path, issue_ref, write),
        Command::ReviewPass {
            workflow_path,
            issue_ref,
            evidence,
            write,
        } => review_manual_pass(workflow_path, issue_ref, evidence, write),
        Command::ReviewReject {
            workflow_path,
            issue_ref,
            evidence,
            target_state,
            write,
        } => review_manual_reject(workflow_path, issue_ref, evidence, target_state, write),
        Command::ReviewSession {
            workflow_path,
            issue_ref,
            write,
        } => {
            legacy_agent_session_start(workflow_path, issue_ref, AgentSessionLaneArg::Review, write)
        }
        Command::ReviewFreshness { input } => review_freshness(input),
        Command::ReviewLoop { options } => review_loop(options),
        Command::ReviewStatus { options } => review_status(options),
        Command::MergeSession {
            workflow_path,
            issue_ref,
            write,
        } => {
            legacy_agent_session_start(workflow_path, issue_ref, AgentSessionLaneArg::Merge, write)
        }
        Command::AgentSessionStart {
            workflow_path,
            issue_ref,
            lane,
            run_id,
            write,
        } => agent_session_start(workflow_path, issue_ref, lane, run_id, write),
        Command::SessionStart {
            workflow_path,
            issue_ref,
            lane,
            run_id,
            write,
        } => agent_session_start(workflow_path, issue_ref, lane, Some(run_id), write),
        Command::SessionList { workflow_path } => agent_session_list(workflow_path),
        Command::SessionAttach {
            workflow_path,
            session,
            exec,
        } => agent_session_attach(workflow_path, session, exec),
        Command::AgentSessionList { workflow_path } => agent_session_list(workflow_path),
        Command::AgentSessionAttach {
            workflow_path,
            session,
            exec,
        } => agent_session_attach(workflow_path, session, exec),
        Command::Gate {
            workflow_path,
            issue_ref,
            apply,
            write,
        } => quality_gate(workflow_path, issue_ref, apply, write),
        Command::ForgeValidate {
            workflow_path,
            status,
            title,
            markdown,
            issue_ref,
        } => forge_validate(workflow_path, status, title, markdown, issue_ref),
        Command::ForgeCreate {
            workflow_path,
            title,
            markdown,
            status,
            project,
            project_fields,
            assignees,
            write,
            dry_run,
        } => forge_create(ForgeCreateOptions {
            workflow_path,
            title,
            markdown,
            status,
            project,
            project_fields,
            assignees,
            write,
            dry_run,
        }),
        Command::ForgePromote {
            workflow_path,
            issue_ref,
            title,
            markdown,
            promotion_note,
            write,
            dry_run,
        } => forge_promote(
            workflow_path,
            issue_ref,
            title,
            markdown,
            promotion_note,
            write,
            dry_run,
        ),
        Command::ForgeRework { options } => forge_rework(options),
        Command::Help(text) => {
            print!("{text}");
            Ok(())
        }
    }
}

fn plan(workflow_path: PathBuf, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = build_plan_snapshot(&workflow_path)?;
    println!("{}", render_plan_snapshot(&snapshot, json)?);

    Ok(())
}

fn autopilot_plan(workflow_path: PathBuf, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = build_autopilot_plan(&workflow_path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        println!("{}", render_autopilot_plan_human(&snapshot));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AutopilotPlanSnapshot {
    schema_version: u8,
    read_only: bool,
    workflow_path: String,
    state_summary: String,
    lanes: Vec<AutopilotLanePlan>,
    parked_queues: Vec<AutopilotParkedQueue>,
    readiness: AutopilotReadiness,
    doctor: AutopilotDoctorSummary,
    canonical_checkout: AutopilotCanonicalCheckout,
    runtime: AutopilotRuntimeSummary,
    integration_gaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AutopilotLanePlan {
    lane: String,
    status: String,
    selected_issue: Option<AutopilotIssueSummary>,
    proposed_action: String,
    target_state: Option<String>,
    reason: String,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AutopilotIssueSummary {
    identifier: String,
    title: String,
    state: String,
    url: Option<String>,
    priority: Option<i64>,
    pull_request: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AutopilotParkedQueue {
    name: String,
    state: String,
    count: usize,
    issues: Vec<AutopilotIssueSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AutopilotReadiness {
    status: String,
    reason: String,
    blockers: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AutopilotDoctorSummary {
    blockers: usize,
    warnings: usize,
    blocker_codes: Vec<String>,
    warning_codes: Vec<String>,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AutopilotCanonicalCheckout {
    safe_for_write: bool,
    root: Option<String>,
    branch: Option<String>,
    upstream: Option<String>,
    clean: Option<bool>,
    reason: Option<String>,
    status_line: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AutopilotRuntimeSummary {
    runtime_state_count: usize,
    session_count: usize,
    session_attention_count: usize,
    blockers: Vec<String>,
    evidence: Vec<String>,
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

struct AutopilotPlanInputs<'a> {
    workflow_path: &'a Path,
    config: &'a RuntimeConfig,
    adapter: &'a dyn TrackerAdapter,
    issues: Vec<TrackerIssue>,
    doctor_report: ProjectAuditReport,
    canonical_checkout: AutopilotCanonicalCheckout,
    runtime: AutopilotRuntimeSummary,
    integration_gaps: Vec<String>,
}

fn build_autopilot_plan_from_parts(
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
    fn from_parts(
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

fn status_api(
    workflow_path: PathBuf,
    bind: SocketAddr,
    once: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !once {
        return Err("status serve currently requires --once".into());
    }
    if !bind.ip().is_loopback() {
        return Err("status serve bind address must be loopback for this first slice".into());
    }

    let snapshot = build_plan_snapshot(&workflow_path)?;
    println!("status_api=serving bind={bind} mode=once");
    let local_addr = serve_once(bind, &snapshot)?;
    println!("status_api=stopped bind={local_addr} mode=once");
    Ok(())
}

fn build_plan_snapshot(
    workflow_path: &Path,
) -> Result<jade_symphony::model::RuntimeSnapshot, Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(workflow_path);
    let workflow = WorkflowDefinition::load(workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let integration_gaps = adapter.integration_gaps();
    let issues = adapter.list_project_summary_issues()?;
    let session_statuses = session_status_snapshots(&config);
    let event_log_path = config
        .observability
        .logs_root
        .join("jade-symphony.jsonl")
        .display()
        .to_string();
    let orchestrator = Orchestrator::new(config);
    let mut plan = orchestrator.plan_dispatch(issues);
    plan.integration_gaps.extend(integration_gaps);
    match session_statuses {
        Ok(sessions) => plan.snapshot.sessions = sessions,
        Err(error) => plan
            .integration_gaps
            .push(format!("tmux session status unavailable: {error}")),
    }
    plan.snapshot.integration_gaps = plan.integration_gaps.clone();
    plan.snapshot.event_log_path = Some(event_log_path);
    Ok(plan.snapshot)
}

fn session_status_snapshots(
    config: &RuntimeConfig,
) -> Result<Vec<SessionStatusSnapshot>, Box<dyn std::error::Error>> {
    let registry = load_session_registry(&session_registry_path(config))?;
    let now_ms = unix_timestamp_ms();
    let mut snapshots = Vec::new();

    for record in registry.sessions.iter().rev().take(20).rev() {
        let is_tmux_session = record.backend == "tmux";
        let pane_tail = if is_tmux_session {
            capture_tmux_pane_tail(
                &config.tmux.command,
                &record.pane_target,
                DEFAULT_SESSION_STATUS_LINES,
            )
            .ok()
        } else {
            None
        };
        let log_tail = if is_tmux_session {
            read_log_tail(&record.log_path, DEFAULT_SESSION_STATUS_LINES)?
        } else {
            None
        };
        let probe = classify_session_record(
            record,
            pane_tail.as_deref(),
            log_tail.as_deref(),
            now_ms,
            DEFAULT_SESSION_STALE_AFTER_MS,
        );
        snapshots.push(SessionStatusSnapshot {
            session_id: record.session_name.clone(),
            lane: record.lane.clone(),
            backend: record.backend.clone(),
            run_id: record.run_id.clone(),
            status: probe.status.as_str().into(),
            evidence_source: probe.source.as_str().into(),
            evidence: probe.evidence,
            issue_identifier: record.issue_identifier.clone(),
            issue_title: record.issue_title.clone(),
            attach_command: is_tmux_session.then(|| record.attach_command.clone()),
            log_path: is_tmux_session.then(|| record.log_path.display().to_string()),
            updated_at_ms: record.updated_at_ms,
        });
    }

    Ok(snapshots)
}

fn render_plan_snapshot(
    snapshot: &jade_symphony::model::RuntimeSnapshot,
    json: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    if json {
        Ok(serde_json::to_string_pretty(snapshot)?)
    } else {
        Ok(render_snapshot(snapshot))
    }
}

fn quality_gate(
    workflow_path: PathBuf,
    issue_ref: String,
    apply: bool,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .issue(issue_ref.clone())
            .backend(tracker_backend_label(&config))
            .next("inspect_issue"),
        || adapter.get_issue(&issue_ref),
    )?
    .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let decision = evaluate_issue_for_current_source(&config, &issue)?;

    println!(
        "gate={:?} dispatchable={}",
        decision.kind,
        decision.is_dispatchable()
    );
    if !decision.missing.is_empty() {
        println!("missing={}", decision.missing.join(", "));
    }
    if !decision.assumptions.is_empty() {
        println!("assumptions={}", decision.assumptions.join("; "));
    }

    if apply {
        require_write_intent(write)?;
        let workpad = gate_workpad(&issue, &decision);
        adapter.upsert_workpad(&issue_ref, &workpad)?;
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "forge validate",
                mutation_type: "workpad_write",
                issue_ref: Some(&issue_ref),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "quality gate workpad",
            },
        );
        if !decision.is_dispatchable() {
            let target_state = gate_target_state(&decision);
            adapter.set_state(&issue_ref, target_state)?;
            append_tracker_mutation_audit(
                &config,
                TrackerMutationAudit {
                    command: "project inspect",
                    mutation_type: "state_change",
                    issue_ref: Some(&issue_ref),
                    target: None,
                    from_state: Some(issue.state.clone()),
                    to_state: Some(target_state.into()),
                    reason: "quality gate routing",
                },
            );
            println!("applied=true target_state={target_state}");
        } else {
            println!("applied=true target_state=unchanged");
        }
    }

    Ok(())
}

fn evaluate_issue_for_current_source(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<GateDecision, Box<dyn std::error::Error>> {
    let repo_root = std::env::current_dir()?;
    let expected_target = expected_target_repository(config);
    let deterministic =
        evaluate_issue_with_source_alignment(issue, &repo_root, expected_target.as_deref());
    if let Some(blocker) = live_missing_assignee_gate_blocker(config, issue) {
        return Ok(GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing: vec![blocker],
            assumptions: deterministic.assumptions,
            notes: vec!["Live GitHub dispatch requires explicit issue ownership.".into()],
        });
    }
    let decision = evaluate_issue_with_llm_gate(
        issue,
        deterministic,
        &LlmGateOptions {
            mode: LlmGateMode::parse(&config.quality_gate.llm.mode),
            command: config.quality_gate.llm.command.clone(),
            timeout_ms: config.quality_gate.llm.timeout_ms,
        },
    );
    if !decision.is_dispatchable() {
        return Ok(decision);
    }

    let terminal_states = config.terminal_state_set().into_iter().collect();
    let dependency_preflight = evaluate_issue_with_dependency_preflight(issue, &terminal_states);
    if dependency_preflight.is_dispatchable() {
        Ok(decision)
    } else {
        Ok(dependency_preflight)
    }
}

fn live_missing_assignee_gate_blocker(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Option<String> {
    (live_github_tracker(config)
        && !config.tracker.assignee_filter.allow_unassigned
        && issue.assignees.is_empty())
    .then(|| "live GitHub issue assignee".into())
}

fn expected_target_repository(config: &RuntimeConfig) -> Option<String> {
    Some(format!(
        "{}/{}",
        config.tracker.owner.as_ref()?,
        config.tracker.repo.as_ref()?
    ))
}

fn set_state(
    workflow_path: PathBuf,
    issue_ref: String,
    state: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    if !transition_allowed_for_main_agent(&normalize_state(&state)) {
        return Err("main implementation agent cannot set Human Review".into());
    }
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let initial_issue = adapter.get_issue(&issue_ref)?;
    let from_state = initial_issue
        .as_ref()
        .map(|issue| issue.state.clone())
        .filter(|current| !current.is_empty());
    let outcome = set_state_with_recovery(
        adapter.as_ref(),
        &issue_ref,
        initial_issue.as_ref(),
        &state,
        "state_change",
    )?;
    reconcile_main_handoff_runtime_state(&config, &issue_ref, &state)?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "set-state",
                mutation_type: "state_change",
                issue_ref: Some(&issue_ref),
                target: None,
                from_state,
                to_state: Some(state.clone()),
                reason: "explicit CLI state update",
            },
        );
    }
    println!(
        "set_state={} issue_ref={issue_ref} state={state}",
        outcome.as_str()
    );
    Ok(())
}

fn reconcile_main_handoff_runtime_state(
    config: &RuntimeConfig,
    issue_ref: &str,
    target_state: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if normalize_state(target_state) != "agent_review" {
        return Ok(());
    }

    let now_ms = current_time_ms();
    let registry_path = session_registry_path(config);
    let mut registry = load_session_registry(&registry_path)?;
    let mut completed_sessions = 0usize;

    for record in &mut registry.sessions {
        if record.lane == "main"
            && record
                .issue_identifier
                .as_deref()
                .is_some_and(|identifier| issue_refs_match_local(identifier, issue_ref))
            && !matches!(
                record.status,
                SessionStatus::Completed | SessionStatus::Recorded | SessionStatus::Failed
            )
        {
            record.status = SessionStatus::Completed;
            record.updated_at_ms = now_ms;
            completed_sessions += 1;
        }
    }

    if completed_sessions > 0 {
        save_session_registry(&registry_path, &registry)?;
    }

    let runtime_state = load_runtime_states(config)?.into_iter().find(|state| {
        state
            .active_issue
            .as_ref()
            .is_some_and(|issue| issue_refs_match_local(&issue.identifier, issue_ref))
            && state.lane.as_deref().is_none_or(|lane| lane == "main")
    });
    let runtime_matches_main_issue = runtime_state.is_some();
    if runtime_matches_main_issue {
        remove_runtime_state_for_issue(config, issue_ref)?;
    }

    if completed_sessions > 0 || runtime_matches_main_issue {
        append_runtime_supervision_event(
            config,
            runtime_state.as_ref(),
            "MainHandoffRuntimeReconciled",
            &format!(
                "issue={} target_state=agent_review sessions_completed={} runtime_cleared={}",
                issue_ref, completed_sessions, runtime_matches_main_issue
            ),
        )?;
        println!(
            "main_handoff_runtime_reconcile issue={} sessions_completed={} runtime_cleared={}",
            issue_ref, completed_sessions, runtime_matches_main_issue
        );
    }

    Ok(())
}

fn issue_refs_match_local(left: &str, right: &str) -> bool {
    normalize_issue_ref_local(left) == normalize_issue_ref_local(right)
}

fn normalize_issue_ref_local(value: &str) -> String {
    value.trim().trim_start_matches('#').to_string()
}

fn upsert_workpad(
    workflow_path: PathBuf,
    issue_ref: String,
    markdown_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let markdown = std::fs::read_to_string(&markdown_path)?;
    let initial_issue = adapter.get_issue(&issue_ref)?;
    let key = recovery_key(
        "workpad",
        &issue_ref,
        &format!(
            "{}|{}",
            markdown_path.display(),
            stable_recovery_hash(&markdown)
        ),
    );
    let outcome = upsert_workpad_with_recovery(
        adapter.as_ref(),
        &issue_ref,
        initial_issue.as_ref(),
        &markdown,
        &key,
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "workpad",
                mutation_type: "workpad_write",
                issue_ref: Some(&issue_ref),
                target: Some(markdown_path.display().to_string()),
                from_state: None,
                to_state: None,
                reason: "explicit CLI workpad upsert",
            },
        );
    }
    println!(
        "workpad={} issue_ref={} source={}",
        outcome.as_str(),
        issue_ref,
        markdown_path.display()
    );
    Ok(())
}

fn append_timeline_comment(
    workflow_path: PathBuf,
    issue_ref: String,
    markdown_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let markdown = std::fs::read_to_string(&markdown_path)?;
    if !write {
        println!(
            "timeline_comment_dry_run action=add_issue_comment issue_ref={} source={}",
            issue_ref,
            markdown_path.display()
        );
        return Ok(());
    }

    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let initial_issue = adapter.get_issue(&issue_ref)?;
    let from_state = initial_issue
        .as_ref()
        .map(|issue| issue.state.clone())
        .filter(|current| !current.is_empty());
    let key = recovery_key(
        "timeline-comment",
        &issue_ref,
        &format!(
            "{}|{}",
            markdown_path.display(),
            stable_recovery_hash(&markdown)
        ),
    );
    let outcome = add_timeline_comment_with_recovery(
        adapter.as_ref(),
        &issue_ref,
        initial_issue.as_ref(),
        &markdown,
        &key,
        "timeline_comment",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "timeline-comment",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue_ref),
                target: Some(markdown_path.display().to_string()),
                from_state,
                to_state: None,
                reason: "explicit CLI append-only timeline comment",
            },
        );
    }
    println!(
        "timeline_comment={} issue_ref={} source={}",
        outcome.as_str(),
        issue_ref,
        markdown_path.display()
    );
    Ok(())
}

fn link_pr(
    workflow_path: PathBuf,
    issue_ref: String,
    pr_ref: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !write {
        println!("link_pr_dry_run action=link_pull_request issue_ref={issue_ref} pr_ref={pr_ref}");
        return Ok(());
    }

    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let repaired = link_pr_with_adapter(adapter.as_ref(), &issue_ref, &pr_ref, true)?;
    if repaired {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "link-pr",
                mutation_type: "pr_link",
                issue_ref: Some(&issue_ref),
                target: Some(pr_ref.clone()),
                from_state: None,
                to_state: None,
                reason: "explicit CLI PR link",
            },
        );
    }
    let action = if repaired {
        "repair_comment"
    } else {
        "already_visible"
    };
    println!("link_pr=ok issue_ref={issue_ref} pr_ref={pr_ref} action={action}");
    Ok(())
}

fn link_pr_with_adapter(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    pr_ref: &str,
    write: bool,
) -> Result<bool, TrackerError> {
    if write {
        let linked = adapter.list_linked_pull_requests(issue_ref)?;
        if linked_pull_requests_contain(&linked, pr_ref) {
            return Ok(false);
        }
        adapter.link_pull_request(issue_ref, pr_ref)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn create_follow_up(
    workflow_path: PathBuf,
    title: String,
    body_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let body = std::fs::read_to_string(&body_path)?;
    let issue_id = adapter.create_follow_up_issue(FollowUpIssueInput {
        title,
        body,
        assignees: Vec::new(),
        project_id: None,
        related_issue_ref: None,
        blocked_by_issue_ref: None,
    })?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "create-follow-up",
            mutation_type: "issue_create",
            issue_ref: None,
            target: Some(issue_id.clone()),
            from_state: None,
            to_state: None,
            reason: "explicit CLI follow-up creation",
        },
    );
    println!("create_follow_up=ok issue_id={issue_id}");
    Ok(())
}

#[derive(Debug, Clone)]
struct ForgeCreateOptions {
    workflow_path: PathBuf,
    title: String,
    markdown: String,
    status: ForgeStatusArg,
    project: Option<String>,
    project_fields: Vec<ProjectFieldAssignment>,
    assignees: Vec<String>,
    write: bool,
    dry_run: bool,
}

fn forge_create(options: ForgeCreateOptions) -> Result<(), Box<dyn std::error::Error>> {
    let ForgeCreateOptions {
        workflow_path,
        title,
        markdown,
        status,
        project,
        project_fields,
        assignees,
        write,
        dry_run,
    } = options;
    if write && dry_run {
        return Err("forge create cannot use --write and --dry-run together".into());
    }
    let dry_run = !write || dry_run;
    let config = load_config(&workflow_path)?;
    let project_label = validate_forge_project_selection(&config, project.as_deref())?;
    let assignees = normalize_forge_assignees(assignees);
    if forge_create_requires_assignee(&config, status) && assignees.is_empty() {
        return Err(
            "forge create --status Todo requires --assignee for live GitHub issue creation".into(),
        );
    }
    let report = forge_validation_report(status, &title, &markdown, &config, &assignees)?;
    print_forge_validation(&report);
    if !report.decision.is_dispatchable() {
        return Err(format!(
            "forge create validation failed for status {}; tracker issue was not created",
            status.as_str()
        )
        .into());
    }

    if dry_run {
        println!(
            "forge_create_dry_run=ok status={} project={} title={:?} project_fields={}",
            status.as_str(),
            project_label,
            report.title,
            project_fields.len()
        );
        return Ok(());
    }

    let adapter = adapter_from_config(&config);
    let create_result = write_forge_created_issue(
        &config,
        adapter.as_ref(),
        ForgeCreateWriteInput {
            title: report.title,
            markdown,
            assignees,
            status,
            project_label: &project_label,
            project_fields: &project_fields,
        },
    )?;

    println!(
        "{}",
        render_forge_create_success(&create_result, status, project_fields.len())
    );
    Ok(())
}

struct ForgeCreateWriteInput<'a> {
    title: String,
    markdown: String,
    assignees: Vec<String>,
    status: ForgeStatusArg,
    project_label: &'a str,
    project_fields: &'a [ProjectFieldAssignment],
}

#[derive(Debug, Clone)]
struct ForgeCreateResult {
    issue_id: String,
    readback: Option<TrackerIssue>,
}

fn write_forge_created_issue(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    input: ForgeCreateWriteInput<'_>,
) -> Result<ForgeCreateResult, Box<dyn std::error::Error>> {
    let existing_issues = adapter.list_project_summary_issues()?;
    if let Some(duplicate) = find_duplicate_issue_title(&existing_issues, &input.title) {
        return Err(format!(
            "duplicate tracker issue title detected: {} {}",
            duplicate.identifier,
            duplicate.url.as_deref().unwrap_or(&duplicate.title)
        )
        .into());
    }

    let issue_id = adapter.create_follow_up_issue(FollowUpIssueInput {
        title: input.title,
        body: input.markdown,
        assignees: input.assignees,
        project_id: None,
        related_issue_ref: None,
        blocked_by_issue_ref: None,
    })?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge create",
            mutation_type: "issue_create",
            issue_ref: None,
            target: Some(issue_id.clone()),
            from_state: None,
            to_state: None,
            reason: "quality-gated forge issue creation",
        },
    );

    adapter.add_issue_to_project_with_state(&issue_id, input.status.normalized_state())?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge create",
            mutation_type: "project_add",
            issue_ref: Some(&issue_id),
            target: Some(input.project_label.into()),
            from_state: None,
            to_state: Some(input.status.normalized_state().into()),
            reason: "forge issue added to project with requested initial status",
        },
    );
    for assignment in input.project_fields {
        adapter.set_project_field(&issue_id, assignment)?;
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "forge create",
                mutation_type: "project_field",
                issue_ref: Some(&issue_id),
                target: Some(format!("{}={}", assignment.name, assignment.value)),
                from_state: None,
                to_state: None,
                reason: "forge project field assignment",
            },
        );
    }

    let readback = verify_forge_created_issue_status(adapter, &issue_id, input.status)?;
    Ok(ForgeCreateResult { issue_id, readback })
}

fn verify_forge_created_issue_status(
    adapter: &dyn TrackerAdapter,
    issue_id: &str,
    status: ForgeStatusArg,
) -> Result<Option<TrackerIssue>, Box<dyn std::error::Error>> {
    let expected = normalize_state(status.normalized_state());
    let mut last_state = None;
    let mut last_issue = None;

    for attempt in 0..3 {
        if let Some(issue) = adapter.get_issue(issue_id)? {
            let actual = issue.normalized_state();
            if actual == expected {
                return Ok(Some(issue));
            }
            last_state = Some(issue.state.clone());
            last_issue = Some(issue);
        } else if adapter.kind() == "memory" {
            return Ok(None);
        }

        if attempt < 2 {
            thread::sleep(Duration::from_millis(500));
        }
    }

    if let Some(actual) = last_state {
        Err(format!(
            "forge create stopped at readback: expected Project status {}, got {:?} for {}",
            status.as_str(),
            actual,
            render_forge_created_issue_location(issue_id, last_issue.as_ref())
        )
        .into())
    } else {
        Err(format!(
            "forge create stopped at readback: {} was not found in the configured Project after creation",
            render_forge_created_issue_location(issue_id, None)
        )
        .into())
    }
}

fn render_forge_create_success(
    result: &ForgeCreateResult,
    status: ForgeStatusArg,
    project_fields_count: usize,
) -> String {
    let mut fields = vec![
        "forge_create=ok".to_string(),
        format!("issue_id={}", result.issue_id),
    ];
    if let Some(readback) = result.readback.as_ref() {
        append_forge_created_issue_readback_fields(&mut fields, &result.issue_id, readback);
    }
    fields.push(format!("status={}", status.as_str()));
    if let Some(readback) = result.readback.as_ref() {
        fields.push(format!(
            "project_status={}",
            forge_created_issue_project_status(readback)
        ));
    }
    fields.push(format!("project_fields={project_fields_count}"));
    fields.join(" ")
}

fn render_forge_created_issue_location(issue_id: &str, readback: Option<&TrackerIssue>) -> String {
    let mut fields = vec![format!("issue_id={issue_id}")];
    if let Some(readback) = readback {
        append_forge_created_issue_readback_fields(&mut fields, issue_id, readback);
    }
    fields.join(" ")
}

fn append_forge_created_issue_readback_fields(
    fields: &mut Vec<String>,
    issue_id: &str,
    readback: &TrackerIssue,
) {
    if !readback.identifier.is_empty() && readback.identifier != issue_id {
        fields.push(format!("issue={}", readback.identifier));
    }
    if let Some(url) = readback.url.as_deref().filter(|url| !url.is_empty()) {
        fields.push(format!("url={url}"));
    }
}

fn forge_created_issue_project_status(readback: &TrackerIssue) -> String {
    readback
        .project_fields
        .get("Status")
        .and_then(|status| status.as_str())
        .filter(|status| !status.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| readback.state.clone())
}

fn forge_promote(
    workflow_path: PathBuf,
    issue_ref: String,
    title: String,
    markdown: String,
    promotion_note: PromotionNoteInput,
    write: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if write && dry_run {
        return Err("forge promote cannot use --write and --dry-run together".into());
    }
    let dry_run = !write || dry_run;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let source = adapter
        .get_issue(&issue_ref)
        .map_err(|error| format!("forge promote stopped at read_source: {error}"))?
        .ok_or_else(|| {
            format!("forge promote stopped at read_source: issue not found: {issue_ref}")
        })?;
    if normalize_state(&source.state) != normalize_state(&config.tracker.state_map.backlog) {
        return Err(format!(
            "forge promote stopped at preflight: {} is in {:?}, expected Backlog",
            source.identifier, source.state
        )
        .into());
    }

    let report = forge_validation_report(
        ForgeStatusArg::Todo,
        &title,
        &markdown,
        &config,
        &source.assignees,
    )
    .map_err(|error| format!("forge promote stopped at validate: {error}"))?;
    print_forge_validation(&report);
    if !report.decision.is_dispatchable() {
        return Err("forge promote stopped at validate: promoted body failed Todo gate".into());
    }

    if dry_run {
        let dry_run_readbacks = vec![
            "`forge promote --dry-run` validated the promoted body and promotion note inputs."
                .to_string(),
        ];
        let note = render_promotion_note(
            &source.identifier,
            &report.title,
            &promotion_note,
            &dry_run_readbacks,
        );
        println!(
            "forge_promote_dry_run=ok issue={} from=Backlog to=Todo title={:?}",
            source.identifier, report.title
        );
        println!("promotion_note_preview=\n{note}");
        return Ok(());
    }

    adapter
        .update_issue_content(&source.identifier, &report.title, &markdown)
        .map_err(|error| format!("forge promote stopped at edit_issue: {error}"))?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "forge promote",
            mutation_type: "issue_edit",
            issue_ref: Some(&source.identifier),
            target: Some(report.title.clone()),
            from_state: Some(source.state.clone()),
            to_state: None,
            reason: "forge backlog promotion content update",
        },
    );

    let content_verified = adapter
        .get_issue(&source.identifier)
        .map_err(|error| format!("forge promote stopped at readback: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge promote stopped at readback: issue disappeared after update: {}",
                source.identifier
            )
        })?;
    if content_verified.title != report.title {
        return Err(format!(
            "forge promote stopped at readback: expected title {:?}, got title {:?}",
            report.title, content_verified.title
        )
        .into());
    }

    let write_readbacks = vec![format!(
        "`forge promote --write` updated the existing issue content; pre-status readback confirmed issue `{}` title `{}` before the final Project status mutation.",
        content_verified.identifier, content_verified.title
    )];
    let note = render_promotion_note(
        &source.identifier,
        &content_verified.title,
        &promotion_note,
        &write_readbacks,
    );
    adapter
        .add_issue_comment(&content_verified.identifier, &note)
        .map_err(|error| format!("forge promote stopped at promotion_note: {error}"))?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "forge promote",
            mutation_type: "comment",
            issue_ref: Some(&content_verified.identifier),
            target: Some("Promotion Note".into()),
            from_state: Some(source.state.clone()),
            to_state: None,
            reason: "forge backlog promotion note",
        },
    );

    adapter
        .set_state(&source.identifier, "todo")
        .map_err(|error| format!("forge promote stopped at set_status: {error}"))?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "forge promote",
            mutation_type: "status",
            issue_ref: Some(&source.identifier),
            target: Some("Todo".into()),
            from_state: Some(source.state.clone()),
            to_state: Some("todo".into()),
            reason: "forge backlog promotion final status update",
        },
    );

    let verified = adapter
        .get_issue(&source.identifier)
        .map_err(|error| format!("forge promote stopped at final_readback: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge promote stopped at final_readback: issue disappeared after update: {}",
                source.identifier
            )
        })?;
    let status_ok =
        normalize_state(&verified.state) == normalize_state(&config.tracker.state_map.todo);
    let title_ok = verified.title == report.title;
    if !status_ok || !title_ok {
        return Err(format!(
            "forge promote stopped at final_readback: expected title {:?} and Todo, got title {:?} and state {:?}",
            report.title, verified.title, verified.state
        )
        .into());
    }

    println!(
        "forge_promote=ok issue={} status=Todo title={:?} promotion_note=commented final_status_mutation=true",
        verified.identifier, verified.title
    );
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForgeReworkOptions {
    workflow_path: PathBuf,
    issue_ref: String,
    title: String,
    markdown: String,
    evidence: String,
    operator_confirmation: String,
    write: bool,
    dry_run: bool,
}

fn forge_rework(options: ForgeReworkOptions) -> Result<(), Box<dyn std::error::Error>> {
    let ForgeReworkOptions {
        workflow_path,
        issue_ref,
        title,
        markdown,
        evidence,
        operator_confirmation,
        write,
        dry_run,
    } = options;
    if write && dry_run {
        return Err("forge rework cannot use --write and --dry-run together".into());
    }
    let dry_run = !write || dry_run;
    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(&config, "forge rework", write)?;
    let adapter = adapter_from_config(&config);
    forge_rework_with_adapter(
        &config,
        adapter.as_ref(),
        ForgeReworkInput {
            issue_ref,
            title,
            markdown,
            evidence,
            operator_confirmation,
            dry_run,
        },
    )
}

#[derive(Debug, Clone)]
struct ForgeReworkInput {
    issue_ref: String,
    title: String,
    markdown: String,
    evidence: String,
    operator_confirmation: String,
    dry_run: bool,
}

fn forge_rework_with_adapter(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    input: ForgeReworkInput,
) -> Result<(), Box<dyn std::error::Error>> {
    let confirmation = clean_rework_text(&input.operator_confirmation, "--operator-confirmation")?;
    let evidence = clean_rework_text(&input.evidence, "--evidence-file")?;
    let source = adapter
        .get_issue(&input.issue_ref)
        .map_err(|error| format!("forge rework stopped at read_source: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge rework stopped at read_source: issue not found: {}",
                input.issue_ref
            )
        })?;
    if normalize_state(&source.state) != normalize_state(&config.tracker.state_map.human_review) {
        return Err(format!(
            "forge rework stopped at preflight: {} is in {:?}, expected Human Review",
            source.identifier, source.state
        )
        .into());
    }
    if let Err(error) = ensure_no_active_human_review_lane_claims(&source) {
        if !input.dry_run {
            let diagnostic = render_forge_rework_blocked_workpad(&source, &error.to_string());
            adapter
                .add_issue_comment(&source.identifier, &diagnostic)
                .map_err(|write_error| {
                    format!("forge rework stopped at active_claim_diagnostic: {write_error}")
                })?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "forge rework",
                    mutation_type: "timeline_comment",
                    issue_ref: Some(&source.identifier),
                    target: Some("Rework Revision Blocker".into()),
                    from_state: Some(source.state.clone()),
                    to_state: None,
                    reason: "forge human review rework active claim diagnostic",
                },
            );
        }
        return Err(error);
    }

    let report = forge_validation_report(
        ForgeStatusArg::Todo,
        &input.title,
        &input.markdown,
        config,
        &source.assignees,
    )
    .map_err(|error| format!("forge rework stopped at validate: {error}"))?;
    print_forge_validation(&report);
    if !report.decision.is_dispatchable() {
        return Err(
            "forge rework stopped at validate: replacement body failed executable issue gate"
                .into(),
        );
    }

    if input.dry_run {
        let note = render_forge_rework_workpad(
            &source,
            &report.title,
            &confirmation,
            &evidence,
            &[
                "`forge rework --dry-run` validated Human Review source state, lane claims, replacement body, and evidence inputs.".into(),
            ],
        );
        println!(
            "forge_rework_dry_run=ok issue={} from=HumanReview to=Rework title={:?}",
            source.identifier, report.title
        );
        println!("rework_evidence_preview=\n{note}");
        return Ok(());
    }

    adapter
        .update_issue_content(&source.identifier, &report.title, &input.markdown)
        .map_err(|error| format!("forge rework stopped at edit_issue: {error}"))?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge rework",
            mutation_type: "issue_edit",
            issue_ref: Some(&source.identifier),
            target: Some(report.title.clone()),
            from_state: Some(source.state.clone()),
            to_state: None,
            reason: "forge human review rework content replacement",
        },
    );

    let content_verified = adapter
        .get_issue(&source.identifier)
        .map_err(|error| format!("forge rework stopped at readback: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge rework stopped at readback: issue disappeared after update: {}",
                source.identifier
            )
        })?;
    if content_verified.title != report.title
        || content_verified.description.as_deref() != Some(input.markdown.as_str())
    {
        return Err(format!(
            "forge rework stopped at readback: expected title {:?} and replacement body for {}, got title {:?}",
            report.title, content_verified.identifier, content_verified.title
        )
        .into());
    }

    let readbacks = vec![format!(
        "`forge rework --write` replaced the issue content; pre-status readback confirmed issue `{}` title `{}` before the final Project status mutation.",
        content_verified.identifier, content_verified.title
    )];
    let workpad = render_forge_rework_workpad(
        &content_verified,
        &content_verified.title,
        &confirmation,
        &evidence,
        &readbacks,
    );
    adapter
        .add_issue_comment(&content_verified.identifier, &workpad)
        .map_err(|error| format!("forge rework stopped at evidence_comment: {error}"))?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge rework",
            mutation_type: "timeline_comment",
            issue_ref: Some(&content_verified.identifier),
            target: Some("Rework Revision Evidence".into()),
            from_state: Some(source.state.clone()),
            to_state: None,
            reason: "forge human review rework evidence before status change",
        },
    );

    adapter
        .set_state(&source.identifier, "rework")
        .map_err(|error| format!("forge rework stopped at set_status: {error}"))?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "forge rework",
            mutation_type: "status",
            issue_ref: Some(&source.identifier),
            target: Some("Rework".into()),
            from_state: Some(source.state.clone()),
            to_state: Some("rework".into()),
            reason: "forge human review rework final status update",
        },
    );

    let verified = adapter
        .get_issue(&source.identifier)
        .map_err(|error| format!("forge rework stopped at final_readback: {error}"))?
        .ok_or_else(|| {
            format!(
                "forge rework stopped at final_readback: issue disappeared after update: {}",
                source.identifier
            )
        })?;
    if normalize_state(&verified.state) != normalize_state(&config.tracker.state_map.rework) {
        return Err(format!(
            "forge rework stopped at final_readback: expected Rework, got {:?}",
            verified.state
        )
        .into());
    }

    println!(
        "forge_rework=ok issue={} status=Rework title={:?} evidence=workpad final_status_mutation=true",
        verified.identifier, verified.title
    );
    Ok(())
}

fn clean_rework_text(value: &str, field: &str) -> Result<String, Box<dyn std::error::Error>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("forge rework requires non-empty {field}").into())
    } else {
        Ok(trimmed.to_string())
    }
}

fn ensure_no_active_human_review_lane_claims(
    issue: &TrackerIssue,
) -> Result<(), Box<dyn std::error::Error>> {
    for (field, lane) in [
        ("Main Agent", LaneClaimLane::Main),
        ("Review Agent", LaneClaimLane::Review),
        ("Merging Agent", LaneClaimLane::Merge),
    ] {
        let Some(value) = project_text_field(issue, field) else {
            continue;
        };
        let claim = LaneClaim::parse(&value).map_err(|error| {
            format!(
                "forge rework stopped at preflight: Human Review has unparseable {field} claim: {error}"
            )
        })?;
        if claim.lane != lane {
            return Err(format!(
                "forge rework stopped at preflight: Human Review has mismatched {field} claim lane={}",
                claim.lane.as_str()
            )
            .into());
        }
        if !claim.state.is_terminal_audit_pointer() {
            return Err(format!(
                "forge rework stopped at preflight: Human Review has active {field} claim run={} state={}",
                claim.run,
                claim.state.as_str()
            )
            .into());
        }
    }
    Ok(())
}

fn render_forge_rework_workpad(
    issue: &TrackerIssue,
    rework_title: &str,
    operator_confirmation: &str,
    evidence: &str,
    generated_readbacks: &[String],
) -> String {
    let mut lines = vec![
        "## Jade Symphony Rework Run".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `main`".into(),
        "- Actor role: `human_review_revision`".into(),
        "- Actor: `operator`".into(),
        "- Run ID: `forge-rework`".into(),
        "- Run type: `human_review_rework_revision`".into(),
        "- Input state: `Human Review`".into(),
        "- Target state after run: `Rework`".into(),
        "- Result: `rework_revision_recorded`".into(),
        format!("- PR: `{}`", timeline_pr_summary(issue)),
        format!("- Replacement Rework title/status: `{rework_title}` / `Rework`"),
        format!("- Operator confirmation: {operator_confirmation:?}"),
        "- Evidence summary: operator confirmation, replacement contract, and readback evidence recorded.".into(),
        "- Source state validated as `Human Review` before mutation.".into(),
        "- Terminal lane claims, when present, were preserved as audit pointers.".into(),
        "- Active lane claims in `Human Review` are rejected before content or status writes."
            .into(),
        "- Replacement body was written and read back before the final Project status mutation."
            .into(),
        "- Final Project status mutation is `Rework`.".into(),
        String::new(),
        "### Rework Direction".into(),
        String::new(),
        evidence.trim().to_string(),
        String::new(),
        "### Verification Readback".into(),
        String::new(),
    ];
    push_markdown_bullets(&mut lines, generated_readbacks);
    lines.extend([
        String::new(),
        "### Role Boundary".into(),
        String::new(),
        "- Main Agent may claim `Rework`, repair the revised contract, and stop at `Agent Review`."
            .into(),
        "- `Human Review` remains reserved for independent Review Agent pass evidence.".into(),
    ]);
    lines.join("\n")
}

fn render_forge_rework_blocked_workpad(issue: &TrackerIssue, reason: &str) -> String {
    [
        "## Jade Symphony Rework Run".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `main`".into(),
        "- Actor role: `human_review_revision`".into(),
        "- Actor: `operator`".into(),
        "- Run ID: `forge-rework`".into(),
        "- Run type: `human_review_rework_revision`".into(),
        "- Source state: `Human Review`".into(),
        "- Target state after run: `unchanged`".into(),
        "- Result: `blocked`".into(),
        format!("- PR: `{}`", timeline_pr_summary(issue)),
        format!("- Blocker: {reason}"),
        "- Evidence summary: blocked rework revision recorded before any state mutation.".into(),
        "- No replacement body was written.".into(),
        "- Project status was not changed to `Rework`.".into(),
        "- Resolve or supersede the active lane claim before retrying `forge rework`.".into(),
    ]
    .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromotionNoteInput {
    operator_confirmation: String,
    decisions: Vec<String>,
    scope_changes: Vec<String>,
    dependencies_context: Vec<String>,
    readback_summaries: Vec<String>,
}

fn render_promotion_note(
    source_issue: &str,
    promoted_title: &str,
    input: &PromotionNoteInput,
    generated_readbacks: &[String],
) -> String {
    let mut lines = vec![
        "## Promotion Note".to_string(),
        String::new(),
        format!("- Source Backlog issue: {source_issue}"),
        format!("- Promoted Todo title/status: `{promoted_title}` / `Todo`"),
        format!(
            "- Operator confirmation: {:?}",
            input.operator_confirmation.trim()
        ),
        String::new(),
        "## Key Operator Decisions".to_string(),
        String::new(),
    ];
    push_markdown_bullets(&mut lines, &input.decisions);
    lines.extend([
        String::new(),
        "## Major Scope Changes From Seed".to_string(),
        String::new(),
    ]);
    push_markdown_bullets(&mut lines, &input.scope_changes);
    lines.extend([
        String::new(),
        "## Dependencies and Context".to_string(),
        String::new(),
    ]);
    push_markdown_bullets(&mut lines, &input.dependencies_context);
    lines.extend([
        String::new(),
        "## Verification Readback".to_string(),
        String::new(),
    ]);
    push_markdown_bullets(&mut lines, generated_readbacks);
    push_markdown_bullets(&mut lines, &input.readback_summaries);
    lines.join("\n")
}

fn push_markdown_bullets(lines: &mut Vec<String>, values: &[String]) {
    for value in values {
        lines.push(format!("- {}", value.trim()));
    }
}

fn forge_validate(
    workflow_path: PathBuf,
    status: Option<ForgeStatusArg>,
    title: String,
    markdown: String,
    issue_ref: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let (status, title, markdown, assignees) = if let Some(issue_ref) = issue_ref {
        let adapter = adapter_from_config(&config);
        let issue = adapter
            .get_issue(&issue_ref)?
            .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
        let status = status.unwrap_or_else(|| forge_status_from_issue(&config, &issue));
        let title = if title.trim().is_empty() {
            issue.title.clone()
        } else {
            title
        };
        let markdown = if markdown.trim().is_empty() {
            issue.description.clone().unwrap_or_default()
        } else {
            markdown
        };
        (status, title, markdown, issue.assignees)
    } else {
        let assignees = issue_contract_assignees(&markdown);
        (
            status.unwrap_or(ForgeStatusArg::Todo),
            title,
            markdown,
            assignees,
        )
    };
    let report = forge_validation_report(status, &title, &markdown, &config, &assignees)?;
    print_forge_validation(&report);
    println!("status={}", status.as_str());
    Ok(())
}

fn issue_contract_assignees(markdown: &str) -> Vec<String> {
    markdown
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim().trim_start_matches('-').trim();
            trimmed
                .strip_prefix("Assignee:")
                .or_else(|| trimmed.strip_prefix("Assignees:"))
        })
        .flat_map(|value| value.split(','))
        .map(|assignee| assignee.trim().trim_start_matches('@').to_string())
        .filter(|assignee| !assignee.is_empty() && !assignee.eq_ignore_ascii_case("none"))
        .collect()
}

fn forge_validation_report(
    status: ForgeStatusArg,
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
    intended_assignees: &[String],
) -> Result<ForgeValidationReport, Box<dyn std::error::Error>> {
    match status {
        ForgeStatusArg::Backlog => Ok(validate_backlog_seed(title, markdown)),
        ForgeStatusArg::Todo => {
            validate_forge_create_report_with_assignees(title, markdown, config, intended_assignees)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForgeMissingCategories {
    candidate_missing: Vec<String>,
    live_context_missing: Vec<String>,
}

fn forge_missing_categories(report: &ForgeValidationReport) -> ForgeMissingCategories {
    let (live_context_missing, candidate_missing): (Vec<_>, Vec<_>) = report
        .decision
        .missing
        .iter()
        .cloned()
        .partition(|missing| is_live_context_missing(missing));
    ForgeMissingCategories {
        candidate_missing,
        live_context_missing,
    }
}

fn is_live_context_missing(missing: &str) -> bool {
    matches!(missing, "live GitHub issue assignee")
}

fn validate_backlog_seed(title: &str, markdown: &str) -> ForgeValidationReport {
    let mut missing = Vec::new();
    if title.trim().is_empty() {
        missing.push("title".into());
    }
    if markdown.trim().chars().count() < 40 {
        missing.push("body with enough context to revisit later".into());
    }
    if !markdown.contains("## Issue Goal") && !markdown.contains("## Issue Context") {
        missing.push("at least one Issue Goal or Issue Context section".into());
    }
    let decision = if missing.is_empty() {
        GateDecision::ready()
    } else {
        GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing,
            assumptions: Vec::new(),
            notes: vec![
                "Backlog seed gate is intentionally lighter than the Todo Issue Quality Gate."
                    .into(),
            ],
        }
    };
    ForgeValidationReport {
        title: title.to_string(),
        question: next_clarification_question(&decision),
        decision,
    }
}

fn validate_forge_project_selection(
    config: &RuntimeConfig,
    project: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    let owner = config
        .tracker
        .project_owner
        .as_deref()
        .unwrap_or("workflow");
    let number = config
        .tracker
        .project_number
        .map(|number| number.to_string())
        .unwrap_or_else(|| "configured".into());
    let configured = format!("{owner}/{number}");
    let Some(project) = project.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(configured);
    };
    if matches!(project, "default" | "workflow") || project == number || project == configured {
        Ok(configured)
    } else {
        Err(format!(
            "forge create --project currently supports the configured workflow Project only ({configured}); got {project:?}"
        )
        .into())
    }
}

fn forge_status_from_issue(config: &RuntimeConfig, issue: &TrackerIssue) -> ForgeStatusArg {
    if normalize_state(&issue.state) == normalize_state(&config.tracker.state_map.backlog) {
        ForgeStatusArg::Backlog
    } else {
        ForgeStatusArg::Todo
    }
}

fn normalize_forge_assignees(assignees: Vec<String>) -> Vec<String> {
    assignees
        .into_iter()
        .map(|assignee| assignee.trim().trim_start_matches('@').to_string())
        .filter(|assignee| !assignee.is_empty())
        .collect()
}

fn forge_create_requires_assignee(config: &RuntimeConfig, status: ForgeStatusArg) -> bool {
    status == ForgeStatusArg::Todo
        && config.tracker.kind == "github_project_v2"
        && config.tracker.fixture_path.is_none()
        && !config.tracker.assignee_filter.allow_unassigned
}

fn find_duplicate_issue_title<'a>(
    issues: &'a [TrackerIssue],
    title: &str,
) -> Option<&'a TrackerIssue> {
    let title_key = normalized_issue_title_key(title);
    issues
        .iter()
        .find(|issue| normalized_issue_title_key(&issue.title) == title_key)
}

fn normalized_issue_title_key(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[cfg(test)]
fn validate_forge_create_contract(
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
    intended_assignees: &[String],
) -> Result<jade_symphony::issue_forge::ForgeValidationReport, String> {
    let report =
        validate_forge_create_report_with_assignees(title, markdown, config, intended_assignees)
            .map_err(|error| format!("source alignment failed: {error}"))?;
    if report.decision.is_dispatchable() {
        Ok(report)
    } else {
        Err("issue forge validation failed; tracker issue was not created".into())
    }
}

fn validate_forge_create_report_with_assignees(
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
    intended_assignees: &[String],
) -> Result<jade_symphony::issue_forge::ForgeValidationReport, Box<dyn std::error::Error>> {
    let issue = TrackerIssue {
        tracker_kind: config.tracker.kind.clone(),
        id: "forge-issue-draft".into(),
        item_id: None,
        identifier: "#draft".into(),
        title: title.into(),
        description: Some(markdown.into()),
        url: None,
        state: config.tracker.state_map.todo.clone(),
        labels: Vec::new(),
        assignees: intended_assignees.to_vec(),
        priority: None,
        branch_name: None,
        linked_pull_requests: Vec::new(),
        blocked_by: Vec::new(),
        project_fields: Default::default(),
        created_at: None,
        updated_at: None,
    };
    let decision = evaluate_issue_for_current_source(config, &issue)?;
    Ok(jade_symphony::issue_forge::ForgeValidationReport {
        title: title.to_string(),
        question: next_clarification_question(&decision),
        decision,
    })
}

fn add_to_project(
    workflow_path: PathBuf,
    issue_id: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    adapter.add_issue_to_project(&issue_id)?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "add-to-project",
            mutation_type: "project_add",
            issue_ref: Some(&issue_id),
            target: Some("Project item".into()),
            from_state: None,
            to_state: Some("todo".into()),
            reason: "explicit CLI project add",
        },
    );
    println!("add_to_project=ok issue_id={issue_id}");
    Ok(())
}

fn review_fake(
    workflow_path: PathBuf,
    issue_ref: String,
    outcome: FakeReviewOutcome,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    let adapter = adapter_from_config(&config);
    let issue = run_with_progress_heartbeat(
        progress_spec_with_event_log(&config, "github_project_read")
            .issue(issue_ref.clone())
            .backend(tracker_backend_label(&config))
            .next("review_issue_read"),
        || adapter.get_issue(&issue_ref),
    )?
    .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let request = ReviewRequest {
        issue: issue.clone(),
        prompt: render_automatic_review_prompt(&workflow, &issue)?,
        workspace: config.workspace.root.clone(),
        artifact_root: config.observability.logs_root.join("reviews"),
    };
    let backend = FakeReviewBackend::new(outcome);
    let job = backend.poll(backend.start(request)?)?;
    apply_review_result(
        &config,
        adapter.as_ref(),
        &issue_ref,
        &issue,
        &job,
        None,
        None,
    )?;

    let decision = review_gate_decision_for_issue(&job, &issue);
    println!(
        "review_fake=ok issue_ref={issue_ref} outcome={:?} target_state={:?}",
        decision.outcome, decision.target_state
    );
    println!("{}", decision.message);
    Ok(())
}

fn review_once(
    workflow_path: PathBuf,
    issue_ref: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let request = ReviewRequest {
        issue: issue.clone(),
        prompt: render_automatic_review_prompt(&workflow, &issue)?,
        workspace: config.workspace.root.clone(),
        artifact_root: config.observability.logs_root.join("reviews"),
    };
    let job = match config.review.backend.as_str() {
        "gemini-cli" => {
            let backend = GeminiCliReviewBackend::with_headless_options(
                config.review.gemini_command.clone(),
                config.review.gemini_model.clone(),
                config.review.gemini_allowed_tools.clone(),
            );
            match backend.start(request) {
                Ok(job) => {
                    let spec = review_backend_progress_spec(&config, &issue, backend.kind(), &job);
                    run_with_progress_heartbeat(spec, || {
                        poll_review_job_until_terminal(
                            &backend,
                            job,
                            Duration::from_millis(config.review.timeout_ms),
                            Duration::from_millis(500),
                        )
                    })?
                }
                Err(error) => ReviewJob::failed_unavailable(
                    issue.identifier.clone(),
                    "gemini-cli",
                    error.to_string(),
                ),
            }
        }
        _ => {
            let backend = FakeReviewBackend::new(FakeReviewOutcome::Pass);
            backend.poll(backend.start(request)?)?
        }
    };
    apply_review_result(
        &config,
        adapter.as_ref(),
        &issue_ref,
        &issue,
        &job,
        None,
        None,
    )?;

    let decision = review_gate_decision_for_issue(&job, &issue);
    println!(
        "review_once=ok issue_ref={issue_ref} backend={} outcome={:?} target_state={:?}",
        job.backend, decision.outcome, decision.target_state
    );
    println!("{}", decision.message);
    Ok(())
}

fn review_claim(
    workflow_path: PathBuf,
    issue_ref: String,
    worker: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(&config, "review claim", write)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    if issue.normalized_state() != "agent review" {
        return Err(format!(
            "review claim requires Agent Review state; {} is currently {}",
            issue.identifier, issue.state
        )
        .into());
    }
    let claim = lane_claim_for_issue(
        &issue,
        LaneClaimLane::Review,
        if worker.to_ascii_lowercase().contains("gemini") {
            LaneClaimActor::Gemini
        } else {
            LaneClaimActor::Codex
        },
        LaneClaimSource::Manual,
        project_text_field(&issue, "Review Agent").as_deref(),
    )
    .with_worker(&worker);
    let claim_value = render_parseable_lane_claim(&claim)?;
    if !write {
        println!(
            "review_claim_dry_run action=claim_field issue_ref={} field=\"Review Agent\" value={claim_value}",
            issue.identifier
        );
        return Ok(());
    }
    let outcome = set_project_field_with_recovery(
        adapter.as_ref(),
        &issue,
        &ProjectFieldAssignment {
            name: "Review Agent".into(),
            value: claim_value.clone(),
        },
        "claim_field",
    )?;
    let registry_path = record_manual_lane_claim_evidence(
        &config,
        &issue,
        AgentSessionLaneArg::Review,
        &claim,
        &claim_value,
        &worker,
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "review claim",
                mutation_type: "claim_field",
                issue_ref: Some(&issue.identifier),
                target: Some(format!("Review Agent={claim_value}")),
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "manual review agent claim",
            },
        );
    }
    println!(
        "review_claim={} issue_ref={} field=\"Review Agent\" run={} registry={} value={claim_value}",
        outcome.as_str(),
        issue.identifier,
        claim.run,
        registry_path.display()
    );
    Ok(())
}

fn lane_claim_command(
    workflow_path: PathBuf,
    issue_ref: String,
    lane: AgentSessionLaneArg,
    worker: String,
    source: CliLaneClaimSource,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if worker.trim().is_empty() {
        return Err("lane claim requires a non-empty --worker".into());
    }

    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(
        &config,
        &format!("{} claim", lane.label()),
        write,
    )?;

    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    validate_lane_claim_state(&issue, lane, &config)?;

    let existing_value = project_text_field(&issue, lane.claim_field());
    let claim = lane_claim_for_manual_worker(
        &issue,
        lane,
        actor_from_worker(&worker),
        source.into(),
        worker.trim(),
        existing_value.as_deref(),
    )?;
    let claim_value = render_parseable_lane_claim(&claim)?;

    if !write {
        println!(
            "{}_claim_dry_run action=claim_field issue_ref={} field={:?} run={} value={claim_value}",
            lane.label(),
            issue.identifier,
            lane.claim_field(),
            claim.run
        );
        return Ok(());
    }

    let outcome = set_project_field_with_recovery(
        adapter.as_ref(),
        &issue,
        &ProjectFieldAssignment {
            name: lane.claim_field().into(),
            value: claim_value.clone(),
        },
        "claim_field",
    )?;
    let registry_path =
        record_manual_lane_claim_evidence(&config, &issue, lane, &claim, &claim_value, &worker)?;
    let command_name = format!("{} claim", lane.label());
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: &command_name,
                mutation_type: "claim_field",
                issue_ref: Some(&issue.identifier),
                target: Some(format!("{}={claim_value}", lane.claim_field())),
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "manual lane worker claim",
            },
        );
    }
    println!(
        "{}_claim={} issue_ref={} field={:?} worker={} run={} registry={} value={claim_value}",
        lane.label(),
        outcome.as_str(),
        issue.identifier,
        lane.claim_field(),
        worker.trim(),
        claim.run,
        registry_path.display()
    );
    Ok(())
}

fn validate_lane_claim_state(
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let normalized = issue.normalized_state();
    let valid = match lane {
        AgentSessionLaneArg::Main => {
            matches!(normalized.as_str(), "todo" | "in progress" | "rework")
        }
        AgentSessionLaneArg::Review => normalized == "agent review",
        AgentSessionLaneArg::Merge => normalized == "merging",
    };
    if valid {
        if matches!(lane, AgentSessionLaneArg::Main) {
            let terminal_states = config.terminal_state_set().into_iter().collect();
            if let Some(reason) = native_subissue_gate_blocker(issue, &terminal_states) {
                return Err(format!(
                    "{} claim cannot claim {}; {reason}",
                    lane.label(),
                    issue.identifier
                )
                .into());
            }
        }
        return Ok(());
    }

    Err(format!(
        "{} claim cannot claim {}; {} is currently {}",
        lane.label(),
        issue.identifier,
        issue.identifier,
        issue.state
    )
    .into())
}

fn actor_from_worker(worker: &str) -> LaneClaimActor {
    let normalized = worker.to_ascii_lowercase();
    if normalized.contains("gemini") {
        LaneClaimActor::Gemini
    } else if normalized.contains("claude") {
        LaneClaimActor::Claude
    } else if normalized.contains("human") {
        LaneClaimActor::Human
    } else {
        LaneClaimActor::Codex
    }
}

fn lane_claim_for_manual_worker(
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    actor: LaneClaimActor,
    source: LaneClaimSource,
    worker: &str,
    existing: Option<&str>,
) -> Result<LaneClaim, Box<dyn std::error::Error>> {
    if let Some(existing) = existing {
        if let Ok(claim) = LaneClaim::parse(existing) {
            if claim.lane == lane.claim_lane()
                && claim.issue == issue.identifier
                && claim.state == LaneClaimState::Active
            {
                if claim.worker.as_deref() == Some(worker) {
                    return Ok(claim);
                }
                return Err(format!(
                    "{} already has an active {} claim owned by {} run={}",
                    issue.identifier,
                    lane.label(),
                    claim.worker.as_deref().unwrap_or(claim.actor.as_str()),
                    claim.run
                )
                .into());
            }
        } else if !existing.trim().is_empty() {
            return Err(format!(
                "{} already has an unparseable {} claim: {existing}",
                issue.identifier,
                lane.claim_field()
            )
            .into());
        }
    }

    Ok(LaneClaim::active(
        &issue.identifier,
        lane.claim_lane(),
        actor,
        source,
        current_time_ms(),
    )
    .with_worker(worker))
}

fn render_parseable_lane_claim(claim: &LaneClaim) -> Result<String, Box<dyn std::error::Error>> {
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

fn record_manual_lane_claim_evidence(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    claim: &LaneClaim,
    claim_value: &str,
    worker: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let registry_path = session_registry_path(config);
    let now_ms = unix_timestamp_ms();
    let path = std::env::current_dir()?;
    let branch = current_git_branch(&path).ok().flatten();
    let session_name = format!("manual-{}-{}", lane.label(), safe_identifier(&claim.run));
    let record = AgentSessionRecord {
        issue_id: Some(issue.id.clone()),
        issue_identifier: Some(issue.identifier.clone()),
        issue_title: Some(issue.title.clone()),
        lane: lane.label().into(),
        run_id: Some(claim.run.clone()),
        thread: Some(claim.thread.clone()),
        session_source: Some("manual-claim".into()),
        claim_value: Some(claim_value.into()),
        actor_role: Some(claim.actor.as_str().into()),
        actor_label: Some(worker.trim().into()),
        git_author: None,
        profile_id: None,
        instance_name: None,
        worktree: path,
        branch,
        backend: "codex-app-manual".into(),
        session_name,
        pane_target: String::new(),
        prompt_artifact_path: PathBuf::new(),
        log_path: PathBuf::new(),
        attach_command: "not a tmux session; manual Codex App evidence only".into(),
        attempt: 1,
        status: SessionStatus::Recorded,
        started_at_ms: now_ms,
        updated_at_ms: now_ms,
    };
    save_session_record(&registry_path, record)?;
    Ok(registry_path)
}

fn review_clear_claim(
    workflow_path: PathBuf,
    issue_ref: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .issue(issue_ref.clone())
            .backend(tracker_backend_label(&config))
            .next("inspect_issue"),
        || adapter.get_issue(&issue_ref),
    )?
    .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    if !write {
        println!(
            "review_clear_claim_dry_run action=clear_claim_field issue_ref={} field=\"Review Agent\"",
            issue.identifier
        );
        return Ok(());
    }
    adapter.clear_project_field(&issue.identifier, "Review Agent")?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "review-clear-claim",
            mutation_type: "claim_field_clear",
            issue_ref: Some(&issue.identifier),
            target: Some("Review Agent".into()),
            from_state: Some(issue.state),
            to_state: None,
            reason: "manual review agent claim clear",
        },
    );
    println!(
        "review_clear_claim=ok issue_ref={} field=\"Review Agent\"",
        issue.identifier
    );
    Ok(())
}

fn review_manual_pass(
    workflow_path: PathBuf,
    issue_ref: String,
    evidence: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(&config, "review pass", write)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    if issue.normalized_state() != "agent review" {
        return Err(format!(
            "manual review pass requires Agent Review state; {} is currently {}",
            issue.identifier, issue.state
        )
        .into());
    }

    let (current_claim_value, current_claim) =
        validate_manual_review_pass_claim(&issue, &evidence)?;
    let terminal_claim_value =
        terminal_review_claim_value(&current_claim, LaneClaimState::Done, "passed");
    let target_state = review_pass_target_state(&issue);
    let workpad = render_manual_review_workpad(
        &issue,
        "passed",
        target_state,
        &evidence,
        true,
        &current_claim_value,
        &terminal_claim_value,
    );
    if !write {
        println!(
            "review_pass_dry_run action=timeline_comment issue_ref={} evidence=manual_review_pass",
            issue.identifier
        );
        println!(
            "review_pass_dry_run action=update_claim_field issue_ref={} field=\"Review Agent\" value={terminal_claim_value}",
            issue.identifier,
        );
        println!(
            "review_pass_dry_run action=set_state issue_ref={} target_state={target_state}",
            issue.identifier
        );
        return Ok(());
    }
    let evidence_key = recovery_key(
        "review-pass",
        &issue.identifier,
        &format!(
            "{}|{}|{}",
            terminal_claim_value,
            target_state,
            stable_recovery_hash(&workpad)
        ),
    );
    let evidence_outcome = add_timeline_comment_with_recovery(
        adapter.as_ref(),
        &issue.identifier,
        Some(&issue),
        &workpad,
        &evidence_key,
        "timeline_comment",
    )?;
    if evidence_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "review pass",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue.identifier),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: Some(target_state.into()),
                reason: "manual review pass evidence",
            },
        );
    }
    write_terminal_review_claim(
        &config,
        adapter.as_ref(),
        &issue.identifier,
        &issue.state,
        &terminal_claim_value,
        "review pass terminal claim evidence",
    )?;
    let state_outcome = set_state_with_recovery(
        adapter.as_ref(),
        &issue.identifier,
        Some(&issue),
        target_state,
        "state_change",
    )?;
    if state_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "review pass",
                mutation_type: "state_change",
                issue_ref: Some(&issue.identifier),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: Some(target_state.into()),
                reason: "manual review pass routing",
            },
        );
    }
    println!(
        "review_pass=ok issue_ref={} target_state={target_state}",
        issue.identifier
    );
    Ok(())
}

fn review_manual_reject(
    workflow_path: PathBuf,
    issue_ref: String,
    evidence: String,
    target_state: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let normalized_target = normalize_state(&target_state);
    if normalized_target == "human_review" {
        return Err("review reject cannot target Human Review".into());
    }
    if !matches!(
        normalized_target.as_str(),
        "agent_review" | "agent review" | "rework" | "need_human_input" | "need human input"
    ) {
        return Err(
            "review reject target must be agent_review, rework, or need_human_input".into(),
        );
    }

    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(&config, "review reject", write)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    if issue.normalized_state() != "agent review" {
        return Err(format!(
            "manual review reject requires Agent Review state; {} is currently {}",
            issue.identifier, issue.state
        )
        .into());
    }

    let (current_claim_value, current_claim) =
        validate_active_manual_review_claim(&issue, &evidence)?;
    let (terminal_state, terminal_result) = reject_terminal_claim_outcome(&normalized_target);
    let terminal_claim_value =
        terminal_review_claim_value(&current_claim, terminal_state, terminal_result);
    let workpad = render_manual_review_workpad(
        &issue,
        "not passed",
        &target_state,
        &evidence,
        false,
        &current_claim_value,
        &terminal_claim_value,
    );
    if !write {
        println!(
            "review_reject_dry_run action=timeline_comment issue_ref={} evidence=manual_review_reject",
            issue.identifier
        );
        println!(
            "review_reject_dry_run action=update_claim_field issue_ref={} field=\"Review Agent\" value={terminal_claim_value}",
            issue.identifier,
        );
        println!(
            "review_reject_dry_run action=set_state issue_ref={} target_state={target_state}",
            issue.identifier
        );
        return Ok(());
    }
    let evidence_key = recovery_key(
        "review-reject",
        &issue.identifier,
        &format!(
            "{}|{}|{}",
            terminal_claim_value,
            target_state,
            stable_recovery_hash(&workpad)
        ),
    );
    let evidence_outcome = add_timeline_comment_with_recovery(
        adapter.as_ref(),
        &issue.identifier,
        Some(&issue),
        &workpad,
        &evidence_key,
        "timeline_comment",
    )?;
    if evidence_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "review reject",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue.identifier),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: Some(target_state.clone()),
                reason: "manual review reject evidence",
            },
        );
    }
    write_terminal_review_claim(
        &config,
        adapter.as_ref(),
        &issue.identifier,
        &issue.state,
        &terminal_claim_value,
        "review reject terminal claim evidence",
    )?;
    let state_outcome = set_state_with_recovery(
        adapter.as_ref(),
        &issue.identifier,
        Some(&issue),
        &target_state,
        "state_change",
    )?;
    if state_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "review reject",
                mutation_type: "state_change",
                issue_ref: Some(&issue.identifier),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: Some(target_state.clone()),
                reason: "manual review reject routing",
            },
        );
    }
    println!(
        "review_reject=ok issue_ref={} target_state={target_state}",
        issue.identifier
    );
    Ok(())
}

fn render_manual_review_workpad(
    issue: &TrackerIssue,
    decision: &str,
    target_state: &str,
    evidence: &str,
    pass: bool,
    current_claim_value: &str,
    terminal_claim_value: &str,
) -> String {
    let mut lines = vec![
        "## Jade Symphony Agent Review Run".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `review`".into(),
        "- Actor role: `review_agent`".into(),
        format!(
            "- Actor: `{}`",
            timeline_claim_actor(current_claim_value).unwrap_or("manual-operator".into())
        ),
        format!(
            "- Run ID: `{}`",
            timeline_claim_run(current_claim_value).unwrap_or("not recorded".into())
        ),
        "- Input state: `Agent Review`".into(),
        "- Reviewer backend: manual-operator".into(),
        format!("- Decision: Manual independent review {decision}."),
        format!("- Target state after review routing: `{target_state}`"),
        format!("- Result: `{}`", if pass { "passed" } else { "rework" }),
        format!("- PR: `{}`", timeline_pr_summary(issue)),
        format!("- Review Agent claim: `{current_claim_value}`"),
        format!("- Terminal Review Agent claim: `{terminal_claim_value}`"),
        "- Evidence summary: manual review evidence captured below.".into(),
        String::new(),
        "### Manual Review Evidence".into(),
        "````md".into(),
    ];
    lines.extend(evidence.trim().lines().map(str::to_string));
    lines.push("````".into());
    if pass {
        lines.push(String::new());
        lines.push("- Review pass evidence: `recorded`".into());
        if normalize_state(target_state) == "merging" {
            lines.push("Evidence recorded. Independent Review Agent may move this native subissue directly to Merging; final Human Review and UAT remain owned by the parent issue.".into());
        } else {
            lines.push("Evidence recorded. Independent Review Agent may move this issue to Human Review; the main implementation agent must not.".into());
        }
    } else {
        lines.push(String::new());
        lines.push(
            "- Review did not pass; unavailable or inconclusive review must not move to Human Review."
                .into(),
        );
    }
    lines.join("\n")
}

fn timeline_claim_run(value: &str) -> Option<String> {
    LaneClaim::parse(value).ok().map(|claim| claim.run)
}

fn timeline_claim_actor(value: &str) -> Option<String> {
    LaneClaim::parse(value)
        .ok()
        .map(|claim| claim.actor.as_str().to_string())
}

fn timeline_pr_summary(issue: &TrackerIssue) -> String {
    issue
        .linked_pull_requests
        .iter()
        .find_map(
            |pull_request| match (pull_request.number, pull_request.url.as_deref()) {
                (Some(number), Some(url)) => Some(format!("#{number} {url}")),
                (Some(number), None) => Some(format!("#{number}")),
                (None, Some(url)) => Some(url.to_string()),
                (None, None) => None,
            },
        )
        .unwrap_or_else(|| "not recorded".into())
}

fn validate_manual_review_pass_claim(
    issue: &TrackerIssue,
    evidence: &str,
) -> Result<(String, LaneClaim), Box<dyn std::error::Error>> {
    let (current, claim) = parse_manual_review_claim(issue)?;
    if claim.state == LaneClaimState::Active
        || (claim.state == LaneClaimState::Done
            && review_claim_result_value(&current) == Some("passed"))
    {
        validate_manual_review_evidence_contains_claim(&current, evidence)?;
        return Ok((current, claim));
    }
    Err(format!(
        "current Review Agent claim must be active, or already state=done result=passed for idempotent pass repair; found state={} result={}",
        claim.state.as_str(),
        review_claim_result_value(&current).unwrap_or("missing")
    )
    .into())
}

fn validate_active_manual_review_claim(
    issue: &TrackerIssue,
    evidence: &str,
) -> Result<(String, LaneClaim), Box<dyn std::error::Error>> {
    let (current, claim) = parse_manual_review_claim(issue)?;
    if claim.state != LaneClaimState::Active {
        return Err(format!(
            "current Review Agent claim must be active before routing, found state={}",
            claim.state.as_str()
        )
        .into());
    }
    validate_manual_review_evidence_contains_claim(&current, evidence)?;
    Ok((current, claim))
}

fn parse_manual_review_claim(
    issue: &TrackerIssue,
) -> Result<(String, LaneClaim), Box<dyn std::error::Error>> {
    let current = project_text_field(issue, "Review Agent")
        .ok_or("manual review routing requires a current Review Agent claim")?;
    let claim = LaneClaim::parse(&current).map_err(|error| {
        format!("current Review Agent claim is not a structured lane claim: {error}")
    })?;
    if claim.lane != LaneClaimLane::Review {
        return Err("current Review Agent claim is not for the review lane".into());
    }
    if claim.issue != issue.identifier {
        return Err(format!(
            "current Review Agent claim issue {} does not match {}",
            claim.issue, issue.identifier
        )
        .into());
    }
    Ok((current, claim))
}

fn validate_manual_review_evidence_contains_claim(
    current: &str,
    evidence: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !evidence.contains(current) {
        return Err(
            "manual review evidence must include the exact current Review Agent claim value".into(),
        );
    }
    Ok(())
}

fn review_claim_result_value(value: &str) -> Option<&str> {
    value
        .split_whitespace()
        .find_map(|token| token.strip_prefix("result="))
}

fn terminal_review_claim_value(claim: &LaneClaim, state: LaneClaimState, result: &str) -> String {
    format!("{} result={result}", claim.with_state(state).render())
}

fn reject_terminal_claim_outcome(normalized_target: &str) -> (LaneClaimState, &'static str) {
    match normalized_target {
        "rework" => (LaneClaimState::Done, "rejected"),
        "need_human_input" | "need human input" => (LaneClaimState::Failed, "blocked"),
        _ => (LaneClaimState::Failed, "inconclusive"),
    }
}

fn write_terminal_review_claim(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    from_state: &str,
    value: &str,
    reason: &'static str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut issue = adapter
        .get_issue(issue_ref)?
        .ok_or_else(|| format!("issue not found before terminal review claim: {issue_ref}"))?;
    issue.state = from_state.into();
    let outcome = set_project_field_with_recovery(
        adapter,
        &issue,
        &ProjectFieldAssignment {
            name: "Review Agent".into(),
            value: value.into(),
        },
        "claim_field",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "manual review routing",
                mutation_type: "claim_field",
                issue_ref: Some(issue_ref),
                target: Some(format!("Review Agent={value}")),
                from_state: Some(from_state.into()),
                to_state: None,
                reason,
            },
        );
    }
    Ok(())
}

fn review_freshness(input: ReviewFreshnessInput) -> Result<(), Box<dyn std::error::Error>> {
    let report = classify_review_freshness(input);
    println!("review_freshness={:?}", report.decision.kind);
    println!(
        "prior_human_review_valid={}",
        report.decision.prior_human_review_valid
    );
    println!(
        "human_rereview_required={}",
        report.decision.human_rereview_required
    );
    println!(
        "main_agent_target_state={}",
        report.decision.main_agent_target_state
    );
    println!(
        "authorized_next_state={}",
        report
            .decision
            .authorized_next_state
            .as_deref()
            .unwrap_or("none")
    );
    println!("rationale={}", report.decision.rationale);
    println!("\n--- review freshness evidence ---\n");
    println!("{}", render_review_freshness_workpad(&report));
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewLoopFailureMemory {
    signature: String,
    first_job_id: String,
    previous_job_id: String,
    repeat_count: usize,
}

fn review_loop_repeated_failure_evidence(
    memory: &mut BTreeMap<String, ReviewLoopFailureMemory>,
    issue: &TrackerIssue,
    job: &ReviewJob,
) -> Option<ReviewRepeatedFailureEvidence> {
    if !matches!(job.state, ReviewJobState::Failed | ReviewJobState::TimedOut) {
        memory.remove(&review_worker_key(issue, &job.backend));
        return None;
    }

    let worker_key = review_worker_key(issue, &job.backend);
    let signature = review_failure_signature(job)?;
    match memory.get_mut(&worker_key) {
        Some(previous) if previous.signature == signature => {
            previous.repeat_count = previous.repeat_count.saturating_add(1);
            let evidence = ReviewRepeatedFailureEvidence {
                repeat_count: previous.repeat_count,
                first_job_id: previous.first_job_id.clone(),
                previous_job_id: previous.previous_job_id.clone(),
                signature,
            };
            previous.previous_job_id = job.id.clone();
            Some(evidence)
        }
        _ => {
            memory.insert(
                worker_key,
                ReviewLoopFailureMemory {
                    signature,
                    first_job_id: job.id.clone(),
                    previous_job_id: job.id.clone(),
                    repeat_count: 1,
                },
            );
            None
        }
    }
}

fn review_loop_recovery_delay_ms(
    config: &RuntimeConfig,
    job: &ReviewJob,
    repeat_count: usize,
) -> Option<u64> {
    let diagnostic = gemini_review_health_diagnostic(job)?;
    if !diagnostic.is_recoverable() {
        return None;
    }

    let base_delay = diagnostic
        .retry_after_ms
        .unwrap_or(config.polling.interval_ms)
        .max(1);
    let multiplier = if diagnostic.retry_after_ms.is_some() {
        1
    } else {
        let exponent = repeat_count.saturating_sub(1).min(5) as u32;
        2u64.saturating_pow(exponent)
    };
    let cap = config.agent.max_retry_backoff_ms.max(1);
    Some(base_delay.saturating_mul(multiplier).min(cap).max(1))
}

fn review_loop(options: ReviewLoopOptions) -> Result<(), Box<dyn std::error::Error>> {
    let limit = options.iteration_limit();
    let mut iterations = 0usize;
    let mut failure_memory = BTreeMap::<String, ReviewLoopFailureMemory>::new();

    loop {
        if let Some(max) = limit {
            if iterations >= max {
                println!("review_loop=stopped reason=max_iterations iterations={iterations}");
                break;
            }
        }

        iterations += 1;
        let workflow = WorkflowDefinition::load(&options.workflow_path)?;
        let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
        config.validate()?;
        preflight_canonical_checkout_for_write_mode(&config, "review_loop", options.write)?;
        let adapter = adapter_from_config(&config);
        let issues = run_with_progress_heartbeat(
            progress_spec_with_event_log(&config, "github_project_read")
                .backend(tracker_backend_label(&config))
                .next("review_queue_scan"),
            || {
                adapter.fetch_issues_by_states(std::slice::from_ref(
                    &config.tracker.state_map.agent_review,
                ))
            },
        )?;
        let issues = run_with_progress_heartbeat(
            progress_spec_with_event_log(&config, "github_project_read")
                .backend(tracker_backend_label(&config))
                .next("review_hydrate_issues"),
            || hydrate_issues_for_review_lane(adapter.as_ref(), issues),
        )?;

        if issues.is_empty() {
            if let Some(delay_ms) = unbounded_loop_sleep_ms(limit, config.polling.interval_ms) {
                println!(
                    "review_loop_idle action=sleep reason=no_agent_review_issue delay_ms={delay_ms} iterations={iterations}"
                );
                thread::sleep(Duration::from_millis(delay_ms));
                continue;
            }
            println!("review_loop=stopped reason=no_agent_review_issue iterations={iterations}");
            break;
        };

        let backend_kind = review_backend_kind(&config, options.fake_outcome.as_ref());
        let selected = select_review_worker_issues(
            &issues,
            &config.tracker.state_map.agent_review,
            &backend_kind,
            options.worker_limit(&config),
        );

        if selected.is_empty() {
            for issue in issues {
                match review_run_eligibility(
                    &issue,
                    &config.tracker.state_map.agent_review,
                    &backend_kind,
                ) {
                    ReviewRunEligibility::AlreadyQueued { worker_key } => {
                        println!(
                            "review_loop_action=skip issue={} reason=review_worker_exists worker_key={worker_key}",
                            issue.identifier
                        );
                    }
                    ReviewRunEligibility::NotInAgentReview { current_state } => {
                        println!(
                            "review_loop_action=skip issue={} reason=state_changed current_state={current_state:?}",
                            issue.identifier
                        );
                    }
                    ReviewRunEligibility::InvalidHandoff { reason } => {
                        println!(
                            "review_loop_action=skip issue={} reason=invalid_handoff detail={reason:?}",
                            issue.identifier
                        );
                        record_review_invalid_handoff(
                            &config,
                            adapter.as_ref(),
                            &issue,
                            &reason,
                            options.write,
                        )?;
                    }
                    ReviewRunEligibility::Eligible { .. } => {}
                }
            }
            if let Some(delay_ms) = unbounded_loop_sleep_ms(limit, config.polling.interval_ms) {
                println!(
                    "review_loop_idle action=sleep reason=no_available_review_worker delay_ms={delay_ms} iterations={iterations}"
                );
                thread::sleep(Duration::from_millis(delay_ms));
            }
            continue;
        }

        let mut pending_review_jobs: Vec<(
            usize,
            TrackerIssue,
            LaneClaim,
            thread::JoinHandle<ReviewJob>,
        )> = Vec::new();

        for (slot, selected_issue) in selected.into_iter().enumerate() {
            let worker_slot = slot + 1;
            match review_run_eligibility(
                &selected_issue,
                &config.tracker.state_map.agent_review,
                &backend_kind,
            ) {
                ReviewRunEligibility::Eligible { worker_key } => {
                    print_latest_status(&latest_status_for_issue(
                        &config,
                        &selected_issue,
                        "review",
                        if options.write { "running" } else { "waiting" },
                        "review_selected",
                        Some("write review timeline and reconcile".into()),
                    ));
                    println!(
                    "review_loop_iteration={iterations} worker_slot={worker_slot} issue={} worker_key={worker_key} mode={}",
                    selected_issue.identifier,
                    if options.write { "write" } else { "dry-run" }
                );
                    if !options.write {
                        println!(
                            "review_loop_dry_run action=start issue={} backend={backend_kind} mode={}",
                            selected_issue.identifier,
                            if backend_kind == "gemini-cli" {
                                "headless"
                            } else {
                                "job"
                            }
                        );
                        if backend_kind == "gemini-cli" {
                            println!(
                                "review_loop_dry_run action=command issue={} command={} args={}",
                                selected_issue.identifier,
                                shell_quote_display(&config.review.gemini_command),
                                gemini_cli_headless_args(
                                    config.review.gemini_model.as_deref(),
                                    &config.review.gemini_allowed_tools,
                                )
                                .join(" ")
                            );
                        }
                        print_review_claim_field_dry_run(&selected_issue, &worker_key);
                        println!(
                            "review_loop_dry_run action=timeline_comment issue={} evidence=review_job",
                            selected_issue.identifier
                        );
                        println!(
                            "review_loop_dry_run action=reconcile issue={} actor=independent_review_agent",
                            selected_issue.identifier
                        );
                        continue;
                    }

                    let latest = run_with_progress_heartbeat(
                        progress_spec_with_event_log(&config, "github_project_read")
                            .issue(selected_issue.identifier.clone())
                            .backend(tracker_backend_label(&config))
                            .next("review_issue_read"),
                        || adapter.get_issue(&selected_issue.identifier),
                    )?
                    .ok_or_else(|| {
                        format!(
                            "issue disappeared before review: {}",
                            selected_issue.identifier
                        )
                    })?;
                    match review_run_eligibility(
                        &latest,
                        &config.tracker.state_map.agent_review,
                        &backend_kind,
                    ) {
                        ReviewRunEligibility::Eligible { worker_key } => {
                            let claim = write_review_claim_field(
                                &config,
                                adapter.as_ref(),
                                &latest,
                                &worker_key,
                            )?;
                            let workflow_for_job = workflow.clone();
                            let config_for_job = config.clone();
                            let issue_for_job = latest.clone();
                            let fake_outcome_for_job = options.fake_outcome.clone();
                            let backend_kind_for_job = backend_kind.clone();
                            println!(
                                "review_loop_action=start issue={} worker_slot={} backend={} mode={}",
                                latest.identifier,
                                worker_slot,
                                backend_kind,
                                if backend_kind == "gemini-cli" { "headless" } else { "job" }
                            );
                            let handle = thread::spawn(move || {
                                run_review_job(
                                    &workflow_for_job,
                                    &config_for_job,
                                    &issue_for_job,
                                    fake_outcome_for_job,
                                )
                                .unwrap_or_else(|error| {
                                    ReviewJob::failed_unavailable(
                                        issue_for_job.identifier.clone(),
                                        backend_kind_for_job,
                                        error.to_string(),
                                    )
                                })
                            });
                            pending_review_jobs.push((worker_slot, latest, claim, handle));
                        }
                        ReviewRunEligibility::AlreadyQueued { worker_key } => {
                            println!(
                            "review_loop_action=skip issue={} reason=review_worker_exists worker_key={worker_key}",
                            latest.identifier
                        );
                        }
                        ReviewRunEligibility::NotInAgentReview { current_state } => {
                            println!(
                            "review_loop_action=skip issue={} reason=state_changed current_state={current_state:?}",
                            latest.identifier
                        );
                        }
                        ReviewRunEligibility::InvalidHandoff { reason } => {
                            println!(
                            "review_loop_action=skip issue={} reason=invalid_handoff detail={reason:?}",
                            latest.identifier
                        );
                            record_review_invalid_handoff(
                                &config,
                                adapter.as_ref(),
                                &latest,
                                &reason,
                                options.write,
                            )?;
                        }
                    }
                }
                ReviewRunEligibility::AlreadyQueued { worker_key } => {
                    println!(
                    "review_loop_action=skip issue={} reason=review_worker_exists worker_key={worker_key}",
                    selected_issue.identifier
                );
                }
                ReviewRunEligibility::NotInAgentReview { current_state } => {
                    println!(
                    "review_loop_action=skip issue={} reason=state_changed current_state={current_state:?}",
                    selected_issue.identifier
                );
                }
                ReviewRunEligibility::InvalidHandoff { reason } => {
                    println!(
                        "review_loop_action=skip issue={} reason=invalid_handoff detail={reason:?}",
                        selected_issue.identifier
                    );
                    record_review_invalid_handoff(
                        &config,
                        adapter.as_ref(),
                        &selected_issue,
                        &reason,
                        options.write,
                    )?;
                }
            }
        }

        for (worker_slot, latest, claim, handle) in pending_review_jobs {
            let mut job = match handle.join() {
                Ok(job) => job,
                Err(_) => ReviewJob::failed_unavailable(
                    latest.identifier.clone(),
                    backend_kind.clone(),
                    "review worker thread panicked",
                ),
            };
            let ledger_path =
                write_review_job_ledger_record(&config.observability.logs_root, &latest, &job)?;
            job.ledger_path = Some(ledger_path.clone());
            let repeat_evidence =
                review_loop_repeated_failure_evidence(&mut failure_memory, &latest, &job);
            apply_review_result(
                &config,
                adapter.as_ref(),
                &latest.identifier,
                &latest,
                &job,
                Some(&claim),
                repeat_evidence.as_ref(),
            )?;
            let decision = review_gate_decision_for_issue(&job, &latest);
            println!(
                "review_loop_action=reconciled issue={} worker_slot={} backend={} outcome={:?} target_state={:?} ledger={}",
                latest.identifier,
                worker_slot,
                job.backend,
                decision.outcome,
                decision.target_state,
                ledger_path.display()
            );
            if let Some(diagnostic) = gemini_review_health_diagnostic(&job) {
                println!(
                    "review_loop_health issue={} category={} recovery_policy={} retry_after_ms={} repeat_count={}",
                    latest.identifier,
                    diagnostic.category.as_str(),
                    diagnostic.recovery_policy.as_str(),
                    diagnostic
                        .retry_after_ms
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".into()),
                    repeat_evidence
                        .as_ref()
                        .map(|evidence| evidence.repeat_count)
                        .unwrap_or(1)
                );
            }
            if !options.once {
                let repeat_count = repeat_evidence
                    .as_ref()
                    .map(|evidence| evidence.repeat_count)
                    .unwrap_or(1);
                if let Some(delay_ms) = review_loop_recovery_delay_ms(&config, &job, repeat_count) {
                    let policy = gemini_review_health_diagnostic(&job)
                        .map(|diagnostic| diagnostic.recovery_policy)
                        .unwrap_or(GeminiReviewRecoveryPolicy::RetryWithBackoff);
                    println!(
                        "review_loop_action=wait issue={} reason=gemini_backend_health policy={} delay_ms={} repeat_count={}",
                        latest.identifier,
                        policy.as_str(),
                        delay_ms,
                        repeat_count
                    );
                    thread::sleep(Duration::from_millis(delay_ms));
                }
            }
        }

        if !options.write {
            let Some(delay_ms) = unbounded_loop_sleep_ms(limit, config.polling.interval_ms) else {
                continue;
            };
            println!(
                "review_loop_idle action=sleep reason=dry_run_would_repeat_without_mutation delay_ms={delay_ms} iterations={iterations}"
            );
            thread::sleep(Duration::from_millis(delay_ms));
        }
    }

    Ok(())
}

fn review_status(options: ReviewStatusCliOptions) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&options.workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
    config.validate()?;
    let adapter = adapter_from_config(&config);
    let issues = if let Some(issue_ref) = &options.issue_filter {
        adapter
            .get_issue(issue_ref)?
            .map(|issue| vec![issue])
            .ok_or_else(|| format!("issue not found: {issue_ref}"))?
    } else {
        let mut states = config.tracker.active_states.clone();
        if !states.iter().any(|state| {
            normalize_state(state) == normalize_state(&config.tracker.state_map.agent_review)
        }) {
            states.push(config.tracker.state_map.agent_review.clone());
        }
        hydrate_issues_for_review_lane(adapter.as_ref(), adapter.fetch_issues_by_states(&states)?)?
    };
    let payload = load_review_status(
        &config,
        &issues,
        &ReviewStatusOptions {
            issue_filter: options.issue_filter.clone(),
            recent_limit: options.recent_limit,
            verbose: options.verbose,
        },
        unix_timestamp_ms(),
    )?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{}", render_review_status_human(&payload, options.verbose));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeOnceOutcome {
    NoMergingIssue,
    DryRun,
    Merged,
    Routed,
    Skipped,
}

fn merge_once(workflow_path: PathBuf, write: bool) -> Result<(), Box<dyn std::error::Error>> {
    merge_once_tick(workflow_path, write, false).map(|_| ())
}

fn merge_loop(options: MergeLoopOptions) -> Result<(), Box<dyn std::error::Error>> {
    let limit = options.iteration_limit();
    let workflow = WorkflowDefinition::load(&options.workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
    config.validate()?;
    let max_concurrent = options.worker_limit(&config);
    let mut stopped = false;
    let mut iteration = 0usize;

    loop {
        if let Some(max) = limit {
            if iteration >= max {
                println!("merge_loop=stopped reason=max_iterations iterations={iteration}");
                break;
            }
        }

        iteration += 1;
        let mut should_sleep = false;
        println!(
            "merge_loop_iteration={} mode={} recover={} max_concurrent={max_concurrent}",
            iteration,
            if options.write { "write" } else { "dry-run" },
            options.recover
        );
        for slot in 1..=max_concurrent {
            match merge_once_tick(
                options.workflow_path.clone(),
                options.write,
                options.recover,
            )? {
                MergeOnceOutcome::NoMergingIssue => {
                    if limit.is_none() {
                        should_sleep = true;
                        println!(
                            "merge_loop_idle action=sleep reason=no_merging_issue delay_ms={} iterations={iteration} slot={slot}",
                            config.polling.interval_ms
                        );
                    } else {
                        println!(
                            "merge_loop=stopped reason=no_merging_issue iterations={iteration} slot={slot}"
                        );
                        stopped = true;
                    }
                    break;
                }
                MergeOnceOutcome::DryRun if !options.write => {
                    println!("merge_loop_action=dry_run_tick iterations={iteration} slot={slot}");
                    if limit.is_none() {
                        should_sleep = true;
                        break;
                    } else if max_concurrent > 1 {
                        println!(
                            "merge_loop=stopped reason=dry_run_would_repeat_without_mutation iterations={iteration}"
                        );
                        stopped = true;
                        break;
                    }
                }
                MergeOnceOutcome::Merged => {
                    println!("merge_loop_action=merged iterations={iteration} slot={slot}");
                    if options.write && config.tracker.fixture_path.is_none() {
                        refresh_canonical_checkout_after_merge(&config)?;
                    }
                }
                MergeOnceOutcome::Routed => {
                    println!("merge_loop_action=routed iterations={iteration} slot={slot}");
                }
                MergeOnceOutcome::Skipped => {
                    println!("merge_loop_action=skipped iterations={iteration} slot={slot}");
                }
                MergeOnceOutcome::DryRun => {}
            }
        }
        if stopped {
            break;
        }
        if should_sleep {
            thread::sleep(Duration::from_millis(config.polling.interval_ms));
        }
    }

    Ok(())
}

fn merge_once_tick(
    workflow_path: PathBuf,
    write: bool,
    recover: bool,
) -> Result<MergeOnceOutcome, Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    if config.tracker.fixture_path.is_none() {
        preflight_canonical_checkout_for_write_mode(&config, "merge_loop", write)?;
    }
    let _merge_prompt = workflow.prompt_for_lane(AgentLane::MergeAgent);

    let adapter = adapter_from_config(&config);
    let merging_state = config.tracker.state_map.merging.clone();
    let mut issues = run_with_progress_heartbeat(
        progress_spec_with_event_log(&config, "github_project_read")
            .backend(tracker_backend_label(&config))
            .next("merge_queue_scan"),
        || adapter.fetch_issues_by_states(std::slice::from_ref(&merging_state)),
    )?;
    if issues.is_empty() {
        println!("merge_once=stopped reason=no_merging_issue");
        return Ok(MergeOnceOutcome::NoMergingIssue);
    }

    issues.sort_by_key(|issue| issue.priority.unwrap_or(i64::MAX));
    let worker_id = worker_identity(&config, WorkerLane::Merging);
    let Some(selected) = select_merge_worker_issues(&issues, &worker_id, 1, &config, recover)
        .into_iter()
        .next()
    else {
        println!("merge_once=stopped reason=no_unclaimed_merging_issue");
        return Ok(MergeOnceOutcome::NoMergingIssue);
    };
    let latest_issue = run_with_progress_heartbeat(
        progress_spec_with_event_log(&config, "github_project_read")
            .issue(selected.issue.identifier.clone())
            .backend(tracker_backend_label(&config))
            .next("merge_issue_read"),
        || adapter.get_issue(&selected.issue.identifier),
    )?;
    let (issue, recovery_reason) = match latest_issue {
        Some(issue) => {
            let recovery_reason = recover
                .then(|| merge_recovery_reason(&issue, &worker_id, &config))
                .flatten();
            (issue, recovery_reason)
        }
        None => {
            let recovery_reason = recover.then_some(selected.recovery_reason).flatten();
            (selected.issue.clone(), recovery_reason)
        }
    };
    if let Some(reason) = recovery_reason.as_deref() {
        println!(
            "merge_loop_recovery_candidate issue={} reason={}",
            issue.identifier, reason
        );
    }
    let eligibility = pool_claim_eligibility(&issue, WorkerLane::Merging, &worker_id, &config);
    if !eligibility.is_claimable() && recovery_reason.is_none() {
        println!(
            "merge_once_action=skipped issue={} reason={}",
            issue.identifier,
            eligibility.skip_reason()
        );
        return Ok(MergeOnceOutcome::Skipped);
    }
    let merge_claim = lane_claim_for_issue(
        &issue,
        WorkerLane::Merging.claim_lane(),
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        project_text_field(&issue, WorkerLane::Merging.claim_field()).as_deref(),
    )
    .with_worker(&worker_id);
    write_lane_claim_field(
        &config,
        adapter.as_ref(),
        &issue,
        WorkerLane::Merging,
        &merge_claim,
        write,
    )?;
    let linked_pull_requests = run_with_progress_heartbeat(
        progress_spec_with_event_log(&config, "github_project_read")
            .issue(issue.identifier.clone())
            .backend(tracker_backend_label(&config))
            .next("linked_pr_read"),
        || adapter.list_linked_pull_requests(&issue.identifier),
    )?;
    let runner = ProcessHandoffCommandRunner;
    let default_expected_base = expected_merge_base_branch(&config);
    let expected_base = expected_merge_base_branch_for_issue(&issue, default_expected_base);
    let status = merge_preflight_status(&config, &issue, &linked_pull_requests, &runner)?;
    let decision = merge_lane_decision(
        &issue,
        &merging_state,
        &expected_base,
        &linked_pull_requests,
        status.as_ref(),
    );

    println!(
        "merge_once issue={} decision={:?} target_state={} write={}",
        issue.identifier,
        decision.kind,
        decision.target_state.unwrap_or("none"),
        write
    );
    print_latest_status(&latest_status_for_issue(
        &config,
        &issue,
        "merge",
        if decision.kind.is_merge_ready() {
            "handoff"
        } else if decision.target_state.is_some() {
            "blocked"
        } else {
            "waiting"
        },
        "merge_decision",
        decision.target_state.map(str::to_string),
    ));
    println!("reason={}", decision.reason);
    if let Some(pr_url) = decision.pr_url.as_deref() {
        println!("pull_request={pr_url}");
    }

    if !write {
        print_merge_dry_run_actions(&decision);
        return Ok(MergeOnceOutcome::DryRun);
    }

    if decision.kind == MergeLaneDecisionKind::StaleBranch {
        let pr_ref = decision
            .pr_url
            .as_deref()
            .ok_or("stale-branch decision missing pull request URL")?;
        let output = update_pull_request_branch(pr_ref, &runner, &std::env::current_dir()?)?;
        if output.status == 0 {
            let workpad = merge_lane_workpad(&issue, &decision, Some(&output));
            let comment_outcome = record_merge_timeline_comment_with_recovery(
                &config,
                adapter.as_ref(),
                &issue,
                &decision,
                &workpad,
                "merge lane stale branch update evidence",
            )?;
            println!(
                "merge_once_action=stale_branch_updated issue={} target_state=merging evidence={}",
                issue.identifier,
                comment_outcome.as_str()
            );
            return Ok(MergeOnceOutcome::Skipped);
        }

        let mut failed_update = decision.clone();
        failed_update.kind = MergeLaneDecisionKind::MergeDirty;
        failed_update.target_state = Some("need_human_input");
        failed_update.reason = format!(
            "safe PR branch update failed with status {}: stdout={} stderr={}",
            output.status,
            single_line(&output.stdout),
            single_line(&output.stderr)
        );
        let workpad = merge_lane_workpad(&issue, &failed_update, Some(&output));
        record_merge_timeline_comment_with_recovery(
            &config,
            adapter.as_ref(),
            &issue,
            &failed_update,
            &workpad,
            "merge lane stale branch update failure evidence",
        )?;
        let state_outcome = set_merge_state_with_recovery(
            &config,
            adapter.as_ref(),
            &issue,
            "need_human_input",
            failed_update.pr_url.clone(),
            "merge lane stale branch update failed",
        )?;
        println!(
            "merge_once_action=routed issue={} target_state=need_human_input outcome={}",
            issue.identifier,
            state_outcome.as_str()
        );
        return Ok(MergeOnceOutcome::Routed);
    }

    if decision.kind == MergeLaneDecisionKind::MergeDirty {
        let pr_ref = decision
            .pr_url
            .as_deref()
            .ok_or("dirty-merge decision missing pull request URL")?;
        let head_ref_name = status
            .as_ref()
            .and_then(|status| status.head_ref_name.as_deref())
            .or_else(|| {
                linked_pull_requests
                    .first()
                    .and_then(|pull_request| pull_request.head_ref_name.as_deref())
            });
        let repair = repair_dirty_pull_request(
            pr_ref,
            head_ref_name,
            &expected_base,
            &runner,
            &std::env::current_dir()?,
            merge_rehearsal_mode(&config, &issue),
        )?;
        if repair.repaired {
            let mut repaired_decision = decision.clone();
            repaired_decision.reason = repair.reason.clone();
            let evidence = mechanical_merge_repair_evidence(&repair, &expected_base);
            let workpad = merge_lane_workpad_with_repair_evidence(
                &issue,
                &repaired_decision,
                Some(&repair.output),
                Some(&evidence),
            );
            let comment_outcome = record_merge_timeline_comment_with_recovery(
                &config,
                adapter.as_ref(),
                &issue,
                &repaired_decision,
                &workpad,
                "merge lane safe conflict repair evidence",
            )?;
            println!(
                "merge_once_action=safe_conflict_repaired issue={} target_state=merging evidence={}",
                issue.identifier,
                comment_outcome.as_str()
            );
            return Ok(MergeOnceOutcome::Skipped);
        }

        if repair.is_agent_repair_eligible() {
            let agent_repair = run_merge_agent_conflict_repair(
                &workflow,
                &config,
                &issue,
                &merge_claim,
                pr_ref,
                head_ref_name.unwrap_or_default(),
                &expected_base,
                &repair,
                &runner,
            )?;
            let mut agent_decision = decision.clone();
            agent_decision.reason = agent_repair.reason.clone();
            agent_decision.target_state = if agent_repair.repaired {
                None
            } else {
                Some("need_human_input")
            };
            let workpad = merge_lane_workpad_with_repair_evidence(
                &issue,
                &agent_decision,
                Some(&agent_repair.output),
                Some(&agent_repair.evidence),
            );
            record_merge_timeline_comment_with_recovery(
                &config,
                adapter.as_ref(),
                &issue,
                &agent_decision,
                &workpad,
                if agent_repair.repaired {
                    "merge lane merge-agent conflict repair evidence"
                } else {
                    "merge lane merge-agent conflict repair failure evidence"
                },
            )?;
            if agent_repair.repaired {
                println!(
                    "merge_once_action=merge_agent_conflict_repaired issue={} target_state=merging backend={} session={}",
                    issue.identifier,
                    agent_repair.backend,
                    agent_repair.session_id.as_deref().unwrap_or("n/a")
                );
                return Ok(MergeOnceOutcome::Skipped);
            }

            let state_outcome = set_merge_state_with_recovery(
                &config,
                adapter.as_ref(),
                &issue,
                "need_human_input",
                agent_decision.pr_url.clone(),
                "merge-agent conflict repair needs human input",
            )?;
            println!(
                "merge_once_action=routed issue={} target_state=need_human_input outcome={}",
                issue.identifier,
                state_outcome.as_str()
            );
            return Ok(MergeOnceOutcome::Routed);
        }

        let mut failed_repair = decision.clone();
        failed_repair.target_state = Some("need_human_input");
        failed_repair.reason = repair.reason.clone();
        let evidence = ineligible_merge_agent_repair_evidence(&repair);
        let workpad = merge_lane_workpad_with_repair_evidence(
            &issue,
            &failed_repair,
            Some(&repair.output),
            Some(&evidence),
        );
        record_merge_timeline_comment_with_recovery(
            &config,
            adapter.as_ref(),
            &issue,
            &failed_repair,
            &workpad,
            "merge lane conflict repair failure evidence",
        )?;
        let state_outcome = set_merge_state_with_recovery(
            &config,
            adapter.as_ref(),
            &issue,
            "need_human_input",
            failed_repair.pr_url.clone(),
            "merge lane conflict repair needs human input",
        )?;
        println!(
            "merge_once_action=routed issue={} target_state=need_human_input outcome={}",
            issue.identifier,
            state_outcome.as_str()
        );
        return Ok(MergeOnceOutcome::Routed);
    }

    if decision.kind.is_merge_ready() {
        let pr_ref = decision
            .pr_url
            .as_deref()
            .ok_or("merge-ready decision missing pull request URL")?;
        let output = if merge_rehearsal_mode(&config, &issue) {
            fixture_merge_output(pr_ref)
        } else {
            let (output, merge_outcome) =
                merge_pull_request_with_recovery(pr_ref, &runner, &std::env::current_dir()?)?;
            println!(
                "merge_once_action=merge_command issue={} pr={} outcome={}",
                issue.identifier,
                pr_ref,
                merge_outcome.as_str()
            );
            output
        };
        let workpad = merge_lane_workpad(&issue, &decision, Some(&output));
        record_done_merge_lane_completion(&config, adapter.as_ref(), &issue, &workpad)?;
        println!(
            "merge_once_action=merged issue={} target_state=done",
            issue.identifier
        );
        return Ok(MergeOnceOutcome::Merged);
    }

    let workpad = merge_lane_workpad(&issue, &decision, None);
    record_merge_timeline_comment_with_recovery(
        &config,
        adapter.as_ref(),
        &issue,
        &decision,
        &workpad,
        "merge lane routing evidence",
    )?;
    if let Some(target_state) = decision.target_state {
        let state_outcome = set_merge_state_with_recovery(
            &config,
            adapter.as_ref(),
            &issue,
            target_state,
            decision.pr_url.clone(),
            "merge lane routing",
        )?;
        if decision.kind == MergeLaneDecisionKind::AlreadyMerged
            && normalize_state(target_state) == "done"
        {
            close_completed_issue(&config, adapter.as_ref(), &issue.identifier, Some(&issue))?;
        }
        println!(
            "merge_once_action=routed issue={} target_state={target_state} outcome={}",
            issue.identifier,
            state_outcome.as_str()
        );
        return Ok(MergeOnceOutcome::Routed);
    } else {
        println!("merge_once_action=skipped issue={}", issue.identifier);
    }

    Ok(MergeOnceOutcome::Skipped)
}

fn refresh_canonical_checkout_after_merge(
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    println!("merge_loop_action=refresh_canonical_checkout reason=post_merge");
    let output = ProcessCommand::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(&root)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "merge loop failed to refresh canonical checkout after merge: status={} stdout={} stderr={}",
            output.status.code().unwrap_or(-1),
            single_line(&String::from_utf8_lossy(&output.stdout)),
            single_line(&String::from_utf8_lossy(&output.stderr))
        )
        .into());
    }
    println!(
        "merge_loop_action=refreshed_canonical_checkout stdout=\"{}\"",
        single_line(&String::from_utf8_lossy(&output.stdout))
    );
    enforce_canonical_checkout_before_write(config, "merge_loop")?;
    Ok(())
}

fn record_merge_timeline_comment_with_recovery(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    decision: &jade_symphony::merge_lane::MergeLaneDecision,
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

fn set_merge_state_with_recovery(
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

fn record_done_merge_lane_completion(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    workpad: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let pr_url = issue
        .linked_pull_requests
        .first()
        .and_then(|pr| pr.url.clone());
    let completion_decision = jade_symphony::merge_lane::MergeLaneDecision {
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
    set_merge_state_with_recovery(config, adapter, issue, "done", pr_url, "merge completed")?;
    close_completed_issue(config, adapter, &issue.identifier, Some(issue))?;
    Ok(())
}

fn close_completed_issue(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
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
        "merge_once_action=closed_issue issue={} outcome={}",
        issue_ref,
        outcome.as_str()
    );
    Ok(())
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn merge_preflight_status(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    linked_pull_requests: &[jade_symphony::model::LinkedPullRequest],
    runner: &ProcessHandoffCommandRunner,
) -> Result<Option<jade_symphony::merge_lane::PullRequestMergeStatus>, Box<dyn std::error::Error>> {
    if linked_pull_requests.len() != 1 {
        return Ok(None);
    }

    let linked = &linked_pull_requests[0];
    let number_ref = linked.number.map(|number| number.to_string());
    let Some(pr_ref) = linked.url.as_deref().or(number_ref.as_deref()) else {
        return Ok(None);
    };

    if config.tracker.fixture_path.is_some() || issue.tracker_kind == "memory" {
        return Ok(pull_request_status_from_linked(linked));
    }

    match fetch_pull_request_status_with_recheck(pr_ref, runner, &std::env::current_dir()?, 2) {
        Ok(status) => Ok(Some(status)),
        Err(error) => {
            eprintln!("merge_preflight_warning={error}");
            Ok(None)
        }
    }
}

fn merge_rehearsal_mode(config: &RuntimeConfig, issue: &TrackerIssue) -> bool {
    config.tracker.fixture_path.is_some() || issue.tracker_kind == "memory"
}

struct MergeAgentConflictRepairOutcome {
    repaired: bool,
    output: CommandOutput,
    evidence: MergeRepairEvidence,
    reason: String,
    backend: String,
    session_id: Option<String>,
}

fn mechanical_merge_repair_evidence(
    repair: &MergeConflictRepairOutcome,
    expected_base: &str,
) -> MergeRepairEvidence {
    MergeRepairEvidence {
        method: "mechanical_git_merge".into(),
        conflict_summary: format!(
            "`git merge --no-edit origin/{expected_base}` completed without content conflicts"
        ),
        resolution_summary: repair.reason.clone(),
        semantic_safety: "No agent-authored changes were needed; Git produced a clean merge commit from the approved PR branch and current base.".into(),
        verification: "`git status --porcelain` was clean after the merge commit; push was attempted only after that clean check.".into(),
        push_evidence: format!(
            "push exit status `{}` stdout=`{}` stderr=`{}`",
            repair.output.status,
            single_line(&repair.output.stdout),
            single_line(&repair.output.stderr)
        ),
        next_state_rationale: "Successful repair stays in `Merging` so a later merge tick rereads GitHub mergeability before landing.".into(),
    }
}

fn ineligible_merge_agent_repair_evidence(
    repair: &MergeConflictRepairOutcome,
) -> MergeRepairEvidence {
    MergeRepairEvidence {
        method: "not_started".into(),
        conflict_summary: repair.reason.clone(),
        resolution_summary:
            "Merge-agent repair was not started because trusted repair preconditions were not met."
                .into(),
        semantic_safety:
            "Without a trusted clean PR worktree and content-conflict evidence, the merge lane cannot prove branch safety."
                .into(),
        verification: "No agent verification ran.".into(),
        push_evidence: "No push attempted.".into(),
        next_state_rationale:
            "Unsafe or untrusted repair preconditions route to `Need Human Input` with one operator question."
                .into(),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_merge_agent_conflict_repair(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    claim: &LaneClaim,
    pr_ref: &str,
    head_ref_name: &str,
    expected_base: &str,
    mechanical_repair: &MergeConflictRepairOutcome,
    runner: &dyn HandoffCommandRunner,
) -> Result<MergeAgentConflictRepairOutcome, Box<dyn std::error::Error>> {
    let Some(worktree_path) = mechanical_repair.worktree_path.as_ref() else {
        return Ok(merge_agent_repair_blocked(
            "missing trusted PR worktree after mechanical content-conflict repair failed",
            mechanical_repair,
        ));
    };

    let clean_after_abort = runner.run(
        "git",
        &["status".into(), "--porcelain".into()],
        worktree_path,
    )?;
    if clean_after_abort.status != 0 || !clean_after_abort.stdout.trim().is_empty() {
        return Ok(merge_agent_repair_blocked(
            "PR worktree was not clean after aborting the failed mechanical merge",
            mechanical_repair,
        ));
    }

    let fetch_ref = format!("origin/{expected_base}");
    let fetch = runner.run(
        "git",
        &["fetch".into(), "origin".into(), expected_base.into()],
        worktree_path,
    )?;
    if fetch.status != 0 {
        return Ok(merge_agent_repair_blocked(
            "merge-agent repair could not refresh the expected base branch",
            mechanical_repair,
        ));
    }

    let conflict_merge = runner.run(
        "git",
        &["merge".into(), "--no-edit".into(), fetch_ref.clone()],
        worktree_path,
    )?;
    if conflict_merge.status == 0 {
        return finish_merge_agent_repaired_branch(
            config,
            issue,
            "mechanical_retry",
            "The second base merge completed before agent edits were needed.",
            "`git merge --no-edit` completed cleanly on retry.",
            "No merge-agent semantic changes were needed.",
            vec!["git merge --no-edit".into()],
            pr_ref,
            head_ref_name,
            runner,
            worktree_path,
            CommandOutput {
                status: 0,
                stdout: conflict_merge.stdout,
                stderr: conflict_merge.stderr,
            },
            "direct-cli".into(),
            None,
        );
    }

    let conflict_files = runner.run(
        "git",
        &[
            "diff".into(),
            "--name-only".into(),
            "--diff-filter=U".into(),
        ],
        worktree_path,
    )?;
    let conflict_summary = if conflict_files.stdout.trim().is_empty() {
        format!(
            "Git reported conflicts while merging `{fetch_ref}`, but no unmerged files were listed."
        )
    } else {
        format!(
            "Conflicted files after merging `{fetch_ref}`: `{}`",
            single_line(&conflict_files.stdout)
        )
    };

    let prompt = merge_agent_conflict_repair_prompt(
        workflow,
        issue,
        claim,
        pr_ref,
        head_ref_name,
        expected_base,
        &conflict_summary,
        &mechanical_repair.output,
    )?;
    let backend_spec = agent_session_backend_spec(config, AgentSessionLaneArg::Merge)?;
    let backend = agent_session_backend(&backend_spec.backend)?;
    let prompt_path = rendered_lane_prompt_artifact_path(
        config,
        issue,
        AgentSessionLaneArg::Merge,
        1,
        &backend_spec.backend,
    );
    let mut prepared = backend.prepare(worktree_path.clone(), prompt, config)?;
    prepared.command = Some(backend_spec.command.clone());
    prepared.prompt_artifact_path = Some(prompt_path.clone());
    prepared.issue_id = Some(issue.id.clone());
    prepared.issue_identifier = Some(issue.identifier.clone());
    prepared.issue_title = Some(issue.title.clone());
    prepared.lane = Some("merge".into());
    prepared.run_id = Some(claim.run.clone());
    prepared.branch_name = Some(head_ref_name.into());
    prepared
        .env
        .insert("JADE_SYMPHONY_AGENT_LANE".into(), "merge".into());
    prepared
        .env
        .insert("JADE_SYMPHONY_RUN_ID".into(), claim.run.clone());
    prepared
        .env
        .insert("JADE_SYMPHONY_CLAIM".into(), claim.render());

    let events = match backend.run(prepared) {
        Ok(events) => events,
        Err(error) => {
            let _ = runner.run("git", &["merge".into(), "--abort".into()], worktree_path);
            return Ok(merge_agent_repair_backend_failed(
                &backend_spec.backend,
                format!("merge-agent backend unavailable: {error}"),
                &conflict_summary,
            ));
        }
    };
    let summary = backend.summarize(&events);
    record_agent_session_events(
        config,
        issue,
        AgentSessionLaneArg::Merge,
        &summary,
        &events,
        &prompt_path,
    )?;

    let agent_text = agent_events_text(&events);
    if !summary.success {
        let _ = runner.run("git", &["merge".into(), "--abort".into()], worktree_path);
        return Ok(merge_agent_repair_backend_failed(
            &summary.backend,
            format!("merge-agent backend did not complete: {}", summary.message),
            &conflict_summary,
        ));
    }
    if merge_agent_requests_human_input(&agent_text) {
        let _ = runner.run("git", &["merge".into(), "--abort".into()], worktree_path);
        return Ok(merge_agent_repair_semantic_uncertainty(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            &agent_text,
        ));
    }
    if !merge_agent_reports_repaired(&agent_text) {
        let _ = runner.run("git", &["merge".into(), "--abort".into()], worktree_path);
        return Ok(merge_agent_repair_semantic_uncertainty(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            "merge-agent completed without the required MERGE_AGENT_DECISION marker",
        ));
    }

    let unresolved = runner.run(
        "git",
        &[
            "diff".into(),
            "--name-only".into(),
            "--diff-filter=U".into(),
        ],
        worktree_path,
    )?;
    if unresolved.status != 0 || !unresolved.stdout.trim().is_empty() {
        return Ok(merge_agent_repair_verification_failed(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            format!(
                "unresolved conflict files remain: `{}`",
                single_line(&unresolved.stdout)
            ),
        ));
    }

    let diff_check = runner.run("git", &["diff".into(), "--check".into()], worktree_path)?;
    if diff_check.status != 0 {
        return Ok(merge_agent_repair_verification_failed(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            format!(
                "`git diff --check` failed: stdout=`{}` stderr=`{}`",
                single_line(&diff_check.stdout),
                single_line(&diff_check.stderr)
            ),
        ));
    }

    let pre_commit_status = runner.run(
        "git",
        &["status".into(), "--porcelain".into()],
        worktree_path,
    )?;
    if pre_commit_status
        .stdout
        .lines()
        .any(|line| line.starts_with("??"))
    {
        return Ok(merge_agent_repair_verification_failed(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            "merge-agent left untracked files in the PR worktree".into(),
        ));
    }

    let add = runner.run("git", &["add".into(), "-A".into()], worktree_path)?;
    if add.status != 0 {
        return Ok(merge_agent_repair_verification_failed(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            "`git add -A` failed after conflict resolution".into(),
        ));
    }
    let merge_head = runner.run(
        "git",
        &[
            "rev-parse".into(),
            "-q".into(),
            "--verify".into(),
            "MERGE_HEAD".into(),
        ],
        worktree_path,
    )?;
    if merge_head.status == 0 {
        let commit = runner.run("git", &["commit".into(), "--no-edit".into()], worktree_path)?;
        if commit.status != 0 {
            return Ok(merge_agent_repair_verification_failed(
                &summary.backend,
                summary.session_id.clone(),
                &conflict_summary,
                format!(
                    "`git commit --no-edit` failed: stdout=`{}` stderr=`{}`",
                    single_line(&commit.stdout),
                    single_line(&commit.stderr)
                ),
            ));
        }
    }

    finish_merge_agent_repaired_branch(
        config,
        issue,
        "merge_agent",
        &conflict_summary,
        &merge_agent_resolution_summary(&agent_text),
        &merge_agent_semantic_safety(&agent_text),
        vec![
            "git diff --name-only --diff-filter=U".into(),
            "git diff --check".into(),
            "git status --porcelain".into(),
        ],
        pr_ref,
        head_ref_name,
        runner,
        worktree_path,
        CommandOutput {
            status: 0,
            stdout: summary.message.clone(),
            stderr: String::new(),
        },
        summary.backend,
        summary.session_id,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_merge_agent_repaired_branch(
    _config: &RuntimeConfig,
    _issue: &TrackerIssue,
    method: &str,
    conflict_summary: &str,
    resolution_summary: &str,
    semantic_safety: &str,
    verification_commands: Vec<String>,
    _pr_ref: &str,
    head_ref_name: &str,
    runner: &dyn HandoffCommandRunner,
    worktree_path: &Path,
    initial_output: CommandOutput,
    backend: String,
    session_id: Option<String>,
) -> Result<MergeAgentConflictRepairOutcome, Box<dyn std::error::Error>> {
    let post_status = runner.run(
        "git",
        &["status".into(), "--porcelain".into()],
        worktree_path,
    )?;
    if post_status.status != 0 || !post_status.stdout.trim().is_empty() {
        return Ok(merge_agent_repair_verification_failed(
            &backend,
            session_id,
            conflict_summary,
            format!(
                "repaired branch was not clean before push: `{}`",
                single_line(&post_status.stdout)
            ),
        ));
    }
    let push = runner.run(
        "git",
        &["push".into(), "origin".into(), head_ref_name.into()],
        worktree_path,
    )?;
    if push.status != 0 {
        return Ok(merge_agent_repair_verification_failed(
            &backend,
            session_id,
            conflict_summary,
            format!(
                "push failed: stdout=`{}` stderr=`{}`",
                single_line(&push.stdout),
                single_line(&push.stderr)
            ),
        ));
    }
    Ok(MergeAgentConflictRepairOutcome {
        repaired: true,
        output: CommandOutput {
            status: 0,
            stdout: format!(
                "{}\n{}",
                single_line(&initial_output.stdout),
                single_line(&push.stdout)
            ),
            stderr: single_line(&push.stderr),
        },
        evidence: MergeRepairEvidence {
            method: method.into(),
            conflict_summary: conflict_summary.into(),
            resolution_summary: resolution_summary.into(),
            semantic_safety: semantic_safety.into(),
            verification: verification_commands.join("; "),
            push_evidence: format!(
                "`git push origin {head_ref_name}` exit status `{}`",
                push.status
            ),
            next_state_rationale: "Successful merge-agent repair stays in `Merging` so the next merge tick rereads GitHub mergeability before landing.".into(),
        },
        reason: "merge-agent repaired the conflicted approved PR branch, verification passed, and the existing branch was pushed".into(),
        backend,
        session_id,
    })
}

fn merge_agent_repair_blocked(
    reason: &str,
    mechanical_repair: &MergeConflictRepairOutcome,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        output: mechanical_repair.output.clone(),
        evidence: MergeRepairEvidence {
            method: "merge_agent_not_started".into(),
            conflict_summary: mechanical_repair.reason.clone(),
            resolution_summary: reason.into(),
            semantic_safety: "Trusted repair preconditions failed before the merge-agent could safely edit files.".into(),
            verification: "No agent verification ran.".into(),
            push_evidence: "No push attempted.".into(),
            next_state_rationale: "Route to `Need Human Input` because the merge lane cannot prove safe branch repair.".into(),
        },
        reason: reason.into(),
        backend: "not-started".into(),
        session_id: None,
    }
}

fn merge_agent_repair_backend_failed(
    backend: &str,
    reason: String,
    conflict_summary: &str,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        output: CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: reason.clone(),
        },
        evidence: MergeRepairEvidence {
            method: "merge_agent_backend".into(),
            conflict_summary: conflict_summary.into(),
            resolution_summary: reason.clone(),
            semantic_safety: "Backend failure prevents semantic-safety proof.".into(),
            verification: "No completed repair verification.".into(),
            push_evidence: "No push attempted.".into(),
            next_state_rationale:
                "Route to `Need Human Input` because the repair backend could not complete safely."
                    .into(),
        },
        reason,
        backend: backend.into(),
        session_id: None,
    }
}

fn merge_agent_repair_semantic_uncertainty(
    backend: &str,
    session_id: Option<String>,
    conflict_summary: &str,
    reason: &str,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        output: CommandOutput {
            status: 1,
            stdout: reason.into(),
            stderr: String::new(),
        },
        evidence: MergeRepairEvidence {
            method: "merge_agent_semantic_uncertainty".into(),
            conflict_summary: conflict_summary.into(),
            resolution_summary: single_line(reason),
            semantic_safety: "The merge-agent did not provide a positive semantic-safety proof.".into(),
            verification: "Repair verification was skipped or incomplete because semantic safety was uncertain.".into(),
            push_evidence: "No push attempted.".into(),
            next_state_rationale: "Route to `Need Human Input` with a concrete semantic-safety question.".into(),
        },
        reason: "merge-agent repair could not prove semantic safety".into(),
        backend: backend.into(),
        session_id,
    }
}

fn merge_agent_repair_verification_failed(
    backend: &str,
    session_id: Option<String>,
    conflict_summary: &str,
    reason: String,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        output: CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: reason.clone(),
        },
        evidence: MergeRepairEvidence {
            method: "merge_agent_verification_failed".into(),
            conflict_summary: conflict_summary.into(),
            resolution_summary: reason.clone(),
            semantic_safety: "Verification failure prevents treating the repair as safe.".into(),
            verification: reason.clone(),
            push_evidence: "No push attempted.".into(),
            next_state_rationale: "Route to `Need Human Input` because the repaired branch was not clean and verified.".into(),
        },
        reason,
        backend: backend.into(),
        session_id,
    }
}

#[allow(clippy::too_many_arguments)]
fn merge_agent_conflict_repair_prompt(
    workflow: &WorkflowDefinition,
    issue: &TrackerIssue,
    claim: &LaneClaim,
    pr_ref: &str,
    head_ref_name: &str,
    expected_base: &str,
    conflict_summary: &str,
    mechanical_output: &CommandOutput,
) -> Result<String, jade_symphony::prompt::PromptError> {
    let mut prompt = render_prompt_with_claim(
        workflow.prompt_for_lane(AgentLane::MergeAgent),
        issue,
        None,
        Some(claim),
    )?;
    prompt.push_str(
        "\n\n## Merge-Agent Conflict Repair Task\n\n\
You are repairing the existing approved PR branch in place. Preserve the intent that already passed Agent Review and Human Review. Resolve only conflicts caused by merging the target base into this PR branch. Do not create a replacement PR, do not switch workspaces, and do not route through Rework.\n\n",
    );
    prompt.push_str(&format!("- Pull request: `{pr_ref}`\n"));
    prompt.push_str(&format!("- Head branch: `{head_ref_name}`\n"));
    prompt.push_str(&format!("- Expected base: `{expected_base}`\n"));
    prompt.push_str(&format!("- Conflict summary: {conflict_summary}\n"));
    prompt.push_str(&format!(
        "- Mechanical merge stderr: `{}`\n",
        single_line(&mechanical_output.stderr)
    ));
    prompt.push_str(
        "\n### Required Output Marker\n\n\
End your final response with one of these exact markers:\n\
- `MERGE_AGENT_DECISION: repaired` only if the resolution preserves reviewed intent and verification can proceed.\n\
- `MERGE_AGENT_DECISION: needs_human_input` if there is semantic uncertainty, unrelated drift, unsafe branch/worktree state, or missing verification confidence.\n\n\
Also include `RESOLUTION_SUMMARY:` and `SEMANTIC_SAFETY:` lines. Leave the merge resolution staged or ready for `git add -A`; the merge lane will commit, verify cleanliness, push, and keep the issue in `Merging` for the next tick.\n",
    );
    Ok(prompt)
}

fn agent_events_text(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Message { text, .. } => Some(text.as_str()),
            AgentEvent::Completed { summary, .. } => Some(summary.as_str()),
            AgentEvent::Failed { error, .. } => Some(error.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn merge_agent_reports_repaired(text: &str) -> bool {
    text.contains("MERGE_AGENT_DECISION: repaired")
}

fn merge_agent_requests_human_input(text: &str) -> bool {
    text.contains("MERGE_AGENT_DECISION: needs_human_input")
        || text.to_ascii_lowercase().contains("semantic uncertainty")
}

fn merge_agent_resolution_summary(text: &str) -> String {
    marker_line(text, "RESOLUTION_SUMMARY:")
        .unwrap_or_else(|| "Merge-agent reported repaired conflict resolution.".into())
}

fn merge_agent_semantic_safety(text: &str) -> String {
    marker_line(text, "SEMANTIC_SAFETY:").unwrap_or_else(|| {
        "Merge-agent reported that reviewed implementation intent was preserved.".into()
    })
}

fn marker_line(text: &str, marker: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(marker).map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn print_merge_dry_run_actions(decision: &jade_symphony::merge_lane::MergeLaneDecision) {
    match decision.kind {
        MergeLaneDecisionKind::ReadyToMerge => {
            println!("merge_once_dry_run action=merge");
            println!("merge_once_dry_run action=timeline_comment evidence=merge_result");
            println!("merge_once_dry_run action=set_state target_state=done");
            println!("merge_once_dry_run action=close_issue");
        }
        MergeLaneDecisionKind::AlreadyMerged => {
            println!("merge_once_dry_run action=timeline_comment evidence=already_merged");
            println!("merge_once_dry_run action=set_state target_state=done");
            println!("merge_once_dry_run action=close_issue");
        }
        MergeLaneDecisionKind::StaleBranch => {
            println!("merge_once_dry_run action=update_pr_branch");
            println!("merge_once_dry_run action=timeline_comment evidence=stale_branch_update");
            println!("merge_once_dry_run action=keep_state target_state=merging");
        }
        MergeLaneDecisionKind::MergeDirty => {
            println!("merge_once_dry_run action=attempt_safe_conflict_repair");
            println!("merge_once_dry_run fallback=attempt_merge_agent_conflict_repair");
            println!("merge_once_dry_run action=timeline_comment evidence=conflict_repair_result");
            println!("merge_once_dry_run fallback=set_state target_state=need_human_input");
        }
        _ => {
            println!("merge_once_dry_run action=timeline_comment evidence=preflight_blocker");
            if let Some(target_state) = decision.target_state {
                println!("merge_once_dry_run action=set_state target_state={target_state}");
            }
        }
    }
}

fn review_backend_kind(config: &RuntimeConfig, fake_outcome: Option<&FakeReviewOutcome>) -> String {
    if fake_outcome.is_some() {
        "fake-reviewer".into()
    } else if config.review.backend == "gemini-cli" {
        "gemini-cli".into()
    } else {
        "fake-reviewer".into()
    }
}

fn record_review_invalid_handoff(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    reason: &str,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let workpad = [
        "## Jade Symphony Agent Review Run".to_string(),
        String::new(),
        "### Agent Review Invalid Handoff".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `review`".to_string(),
        "- Input state: `Agent Review`".to_string(),
        "- Target state after review routing: `unchanged`".to_string(),
        "- Actor role: `review_agent`".to_string(),
        "- Decision: `inconclusive_invalid_handoff`".to_string(),
        format!("- Reason: {reason}"),
        "- Review did not start because the Main Agent handoff invariant is not satisfied.".to_string(),
        "- Draft PRs must be marked ready by the Main Agent lane or an operator-confirmed doctor repair before normal Agent Review.".to_string(),
    ]
    .join("\n");

    if write {
        adapter.add_issue_comment(&issue.identifier, &workpad)?;
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "review loop",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue.identifier),
                target: Some("invalid_handoff".into()),
                from_state: Some(issue.state.clone()),
                to_state: Some(issue.state.clone()),
                reason: "review refused invalid Agent Review handoff",
            },
        );
    } else {
        println!(
            "review_loop_dry_run action=timeline_comment issue={} evidence=invalid_handoff",
            issue.identifier
        );
    }

    Ok(())
}

fn select_review_worker_issues(
    issues: &[TrackerIssue],
    agent_review_state: &str,
    backend_kind: &str,
    max_concurrent: usize,
) -> Vec<TrackerIssue> {
    issues
        .iter()
        .filter(|issue| {
            matches!(
                review_run_eligibility(issue, agent_review_state, backend_kind),
                ReviewRunEligibility::Eligible { .. }
            )
        })
        .take(max_concurrent.max(1))
        .cloned()
        .collect()
}

fn review_claim_for_issue(issue: &TrackerIssue, worker_key: &str) -> LaneClaim {
    lane_claim_for_issue(
        issue,
        LaneClaimLane::Review,
        if worker_key.to_ascii_lowercase().contains("gemini") {
            LaneClaimActor::Gemini
        } else {
            LaneClaimActor::Codex
        },
        LaneClaimSource::Loop,
        project_text_field(issue, "Review Agent").as_deref(),
    )
    .with_worker(worker_key)
}

fn print_review_claim_field_dry_run(issue: &TrackerIssue, worker_key: &str) {
    let claim = review_claim_for_issue(issue, worker_key);
    println!(
        "review_loop_dry_run action=claim_field issue={} field={:?} value={:?}",
        issue.identifier,
        "Review Agent",
        claim.render()
    );
}

fn write_review_claim_field(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    worker_key: &str,
) -> Result<LaneClaim, Box<dyn std::error::Error>> {
    let claim = review_claim_for_issue(issue, worker_key);
    let claim_value = render_parseable_lane_claim(&claim)?;
    let outcome = set_project_field_with_recovery(
        adapter,
        issue,
        &ProjectFieldAssignment {
            name: "Review Agent".into(),
            value: claim_value.clone(),
        },
        "claim_field",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "review loop",
                mutation_type: "claim_field",
                issue_ref: Some(&issue.identifier),
                target: Some(format!("Review Agent={claim_value}")),
                from_state: None,
                to_state: None,
                reason: "review worker claim",
            },
        );
    }
    println!(
        "review_loop_action=claim_field issue={} field=\"Review Agent\" run={} outcome={}",
        issue.identifier,
        claim.run,
        outcome.as_str()
    );
    Ok(claim)
}

fn run_review_job(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    fake_outcome: Option<FakeReviewOutcome>,
) -> Result<ReviewJob, Box<dyn std::error::Error>> {
    let request = ReviewRequest {
        issue: issue.clone(),
        prompt: render_automatic_review_prompt(workflow, issue)?,
        workspace: review_workspace_for_issue(config, issue),
        artifact_root: config.observability.logs_root.join("reviews"),
    };

    if let Some(outcome) = fake_outcome {
        let backend = FakeReviewBackend::new(outcome);
        let job = backend.start(request)?;
        let spec = review_backend_progress_spec(config, issue, backend.kind(), &job);
        return Ok(run_with_progress_heartbeat(spec, || {
            poll_review_job_until_terminal(
                &backend,
                job,
                Duration::from_millis(config.review.timeout_ms),
                Duration::from_millis(250),
            )
        })?);
    }

    match config.review.backend.as_str() {
        "gemini-cli" => {
            if let Some(diagnostic) = gemini_prelaunch_health_diagnostic(
                &config.review.gemini_command,
                config.review.gemini_model.as_deref(),
                &config.review.gemini_allowed_tools,
            ) {
                return Ok(ReviewJob::failed_unavailable(
                    issue.identifier.clone(),
                    "gemini-cli",
                    diagnostic.to_error_message(),
                ));
            }
            let backend = GeminiCliReviewBackend::with_headless_options(
                config.review.gemini_command.clone(),
                config.review.gemini_model.clone(),
                config.review.gemini_allowed_tools.clone(),
            );
            match backend.start(request) {
                Ok(job) => {
                    let spec = review_backend_progress_spec(config, issue, backend.kind(), &job);
                    Ok(run_with_progress_heartbeat(spec, || {
                        poll_review_job_until_terminal(
                            &backend,
                            job,
                            Duration::from_millis(config.review.timeout_ms),
                            Duration::from_millis(500),
                        )
                    })?)
                }
                Err(error) => Ok(ReviewJob::failed_unavailable(
                    issue.identifier.clone(),
                    "gemini-cli",
                    error.to_string(),
                )),
            }
        }
        _ => {
            let backend = FakeReviewBackend::new(FakeReviewOutcome::Pass);
            Ok(backend.poll(backend.start(request)?)?)
        }
    }
}

fn review_backend_progress_spec(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    backend: &str,
    job: &ReviewJob,
) -> ProgressHeartbeatSpec {
    let mut spec = progress_spec_with_event_log(config, "review_backend")
        .issue(issue.identifier.clone())
        .backend(backend)
        .next("waiting_for_child");
    if let Some(path) = &job.artifact_path {
        spec = spec.artifact(path.display().to_string());
    }
    spec
}

fn review_workspace_for_issue(config: &RuntimeConfig, issue: &TrackerIssue) -> PathBuf {
    if let Ok(repo_root) = std::env::current_dir() {
        if let Ok(report) = discover_issue_workspaces(config, issue, &repo_root) {
            if let Some(index) = report.canonical_index {
                if let Some(candidate) = report.candidates.get(index) {
                    if candidate.strength == WorkspaceMatchStrength::Strong
                        && candidate.path.starts_with(&config.workspace.root)
                    {
                        return candidate.path.clone();
                    }
                }
            }
        }
    }

    run_loop_handoff_plan(config, issue)
        .map(|handoff| handoff.workspace_path)
        .unwrap_or_else(|_| config.workspace.root.clone())
}

fn render_automatic_review_prompt(
    workflow: &WorkflowDefinition,
    issue: &TrackerIssue,
) -> Result<String, jade_symphony::prompt::PromptError> {
    let mut prompt = render_prompt(
        workflow.prompt_for_lane(AgentLane::ReviewAgent),
        issue,
        None,
    )?;
    prompt.push_str(
        "\n\n## Automatic Headless Review Boundary\n\n\
This Gemini process is running under Jade Symphony automatic `review loop` or `review once`.\n\
Jade Symphony CLI has already claimed or will own any Review Agent claim, timeline comment write,\n\
issue body update, and Project state transition outside this process.\n\n\
Do not run mutating Jade Symphony or GitHub commands, including `review claim`, `review pass`,\n\
`review reject`, `project set-state`, `project workpad`, `forge`, `gh issue edit`, `gh issue comment`, raw\n\
Project GraphQL mutations, or Project UI changes. Do not activate or follow any manual review\n\
skill that tells you to mutate Project state.\n\n\
Return review evidence in stdout only. Start with exactly one line: `Review Result: PASS`,\n\
`Review Result: REWORK`, or `Review Result: NEEDS_CONTEXT`. Use `PASS` only when there are no\n\
blocking findings. Use `REWORK` only when confirmed implementation defects require Main Agent\n\
changes. Use `NEEDS_CONTEXT` when missing evidence or ambiguity prevents an independent decision.\n\n\
UAT is Human Review-owned unless this issue explicitly asks the Main Agent to implement a UAT\n\
harness, fixture, rehearsal path, or workflow capability. Missing Human-owned UAT execution is\n\
not a confirmed implementation defect and must not by itself produce `Review Result: REWORK`.\n\
Report UAT readiness or Human Review follow-up separately under `Evidence`.\n\n\
Only use `[Confirmed]`, `[Plausible]`, `[Rejected]`, or `[Needs Context]` for actual review\n\
findings. Do not use those bracketed finding tags for positive verification evidence, checklist\n\
items, or things that were implemented correctly; put positive observations under an `Evidence`\n\
heading with plain bullets instead. Leave routing and evidence persistence to the Jade Symphony\n\
wrapper after this process exits.\n",
    );
    Ok(prompt)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum AgentSessionLaneArg {
    Main,
    Review,
    Merge,
}

impl AgentSessionLaneArg {
    fn workflow_lane(self) -> AgentLane {
        match self {
            Self::Main => AgentLane::MainAgent,
            Self::Review => AgentLane::ReviewAgent,
            Self::Merge => AgentLane::MergeAgent,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Review => "review",
            Self::Merge => "merge",
        }
    }

    fn claim_field(self) -> &'static str {
        match self {
            Self::Main => "Main Agent",
            Self::Review => "Review Agent",
            Self::Merge => "Merging Agent",
        }
    }

    fn claim_lane(self) -> LaneClaimLane {
        match self {
            Self::Main => LaneClaimLane::Main,
            Self::Review => LaneClaimLane::Review,
            Self::Merge => LaneClaimLane::Merge,
        }
    }
}

fn agent_session_start(
    workflow_path: PathBuf,
    issue_ref: String,
    lane: AgentSessionLaneArg,
    run_id: Option<String>,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let run_id = run_id.ok_or("session start requires explicit --run <RUN_ID>")?;
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    let backend_spec = agent_session_backend_spec(&config, lane)?;
    preflight_canonical_checkout_for_write_mode(&config, "session start", write)?;

    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let workspace_key = agent_session_workspace_key(&config, &issue, lane)?;
    let prompt_path =
        rendered_lane_prompt_artifact_path(&config, &issue, lane, 1, &backend_spec.backend);
    let claim = matching_lane_claim_for_session(&issue, lane, &run_id)?;

    if !write {
        println!(
            "session_dry_run action=start issue={} lane={} run={} backend={} agent_command={} workspace_key={} prompt_artifact={}",
            issue.identifier,
            lane.label(),
            claim.run,
            backend_spec.backend,
            shell_quote_display(&backend_spec.command),
            workspace_key,
            prompt_path.display()
        );
        return Ok(());
    }

    let started = start_agent_session_with_claim(
        &workflow,
        &config,
        adapter.as_ref(),
        &issue,
        lane,
        &claim,
        "session start",
    )?;

    println!(
        "session_action=started issue={} lane={} run={} backend={} session={} pending_session={} workspace={} prompt_artifact={}",
        issue.identifier,
        lane.label(),
        claim.run,
        started.summary.backend,
        started.summary.session_id.as_deref().unwrap_or("n/a"),
        started.summary.pending_session,
        started.workspace_path.display(),
        started.prompt_path.display()
    );
    if let Some(attach_command) = started.summary.attach_command.as_deref() {
        println!("attach_command={attach_command}");
    }
    if let Some(log_path) = started.summary.log_path.as_ref() {
        println!("log_path={}", log_path.display());
    }
    Ok(())
}

struct AgentSessionStartResult {
    summary: jade_symphony::agent::AgentSummary,
    workspace_path: PathBuf,
    prompt_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentSessionBackendSpec {
    backend: String,
    command: String,
}

fn start_agent_session_with_claim(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    claim: &LaneClaim,
    audit_command: &'static str,
) -> Result<AgentSessionStartResult, Box<dyn std::error::Error>> {
    let backend_spec = agent_session_backend_spec(config, lane)?;
    let workspace_key = agent_session_workspace_key(config, issue, lane)?;
    let prompt_path =
        rendered_lane_prompt_artifact_path(config, issue, lane, 1, &backend_spec.backend);
    let workspace = prepare_workspace(&config.workspace.root, &workspace_key, &config.hooks)?;
    let git_identity = apply_local_git_identity(&workspace.path, &config.identity.git)?;
    let prompt = render_prompt_with_claim(
        workflow.prompt_for_lane(lane.workflow_lane()),
        issue,
        None,
        Some(claim),
    )?;
    let backend = agent_session_backend(&backend_spec.backend)?;
    let mut prepared = backend.prepare(workspace.path.clone(), prompt, config)?;
    prepared.command = Some(backend_spec.command.clone());
    prepared
        .env
        .insert("JADE_SYMPHONY_AGENT_LANE".into(), lane.label().to_string());
    prepared.env.insert(
        "JADE_SYMPHONY_AGENT_COMMAND".into(),
        prepared.command.clone().unwrap_or_default(),
    );
    prepared.env.insert(
        "JADE_SYMPHONY_AGENT_BACKEND".into(),
        backend_spec.backend.clone(),
    );
    if backend_spec.backend == "tmux" {
        prepared.env.insert(
            "JADE_SYMPHONY_TMUX_AGENT_COMMAND".into(),
            prepared.command.clone().unwrap_or_default(),
        );
    }
    prepared.prompt_artifact_path = Some(prompt_path.clone());
    prepared.issue_id = Some(issue.id.clone());
    prepared.issue_identifier = Some(issue.identifier.clone());
    prepared.issue_title = Some(issue.title.clone());
    prepared.lane = Some(lane.label().into());
    prepared.run_id = Some(claim.run.clone());
    prepared
        .env
        .insert("JADE_SYMPHONY_RUN_ID".into(), claim.run.clone());
    prepared
        .env
        .insert("JADE_SYMPHONY_CLAIM".into(), claim.render());
    prepared.attempt = 1;
    prepared.branch_name = current_git_branch(&workspace.path).ok().flatten();

    let events = backend.run(prepared)?;
    let summary = backend.summarize(&events);
    record_agent_session_events(config, issue, lane, &summary, &events, &prompt_path)?;

    let claim_value = claim.render();
    let workpad = agent_session_workpad(AgentSessionWorkpadInput {
        issue,
        lane,
        workspace_path: &workspace.path,
        summary: &summary,
        prompt_path: &prompt_path,
        claim_value: &claim_value,
        agent_command: &backend_spec.command,
        git_identity: &git_identity,
    });
    let (mutation_type, outcome) = if lane == AgentSessionLaneArg::Main {
        let key = recovery_key(
            "session-start-workpad",
            &issue.identifier,
            &format!(
                "{}|{}|{}",
                issue.identifier,
                claim.run,
                stable_recovery_hash(&workpad)
            ),
        );
        (
            "workpad_write",
            upsert_workpad_with_recovery(adapter, &issue.identifier, Some(issue), &workpad, &key)?,
        )
    } else {
        let key = recovery_key(
            "session-start-timeline",
            &issue.identifier,
            &format!(
                "{}|{}|{}",
                issue.identifier,
                claim.run,
                stable_recovery_hash(&workpad)
            ),
        );
        (
            "timeline_comment",
            add_timeline_comment_with_recovery(
                adapter,
                &issue.identifier,
                Some(issue),
                &workpad,
                &key,
                "timeline_comment",
            )?,
        )
    };
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: audit_command,
                mutation_type,
                issue_ref: Some(&issue.identifier),
                target: summary.session_id.clone(),
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "manual lane session evidence",
            },
        );
    }

    Ok(AgentSessionStartResult {
        summary,
        workspace_path: workspace.path,
        prompt_path,
    })
}

fn legacy_agent_session_start(
    _workflow_path: PathBuf,
    _issue_ref: String,
    lane: AgentSessionLaneArg,
    _write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    Err(format!(
        "legacy session aliases are unavailable; use `{} claim` first, then `session start --lane {} --run <RUN_ID>`",
        lane.label(),
        lane.label()
    )
    .into())
}

fn matching_lane_claim_for_session(
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    run_id: &str,
) -> Result<LaneClaim, Box<dyn std::error::Error>> {
    let claim_value = project_text_field(issue, lane.claim_field()).ok_or_else(|| {
        format!(
            "session start requires an existing {} claim for {}",
            lane.claim_field(),
            issue.identifier
        )
    })?;
    let claim = LaneClaim::parse(&claim_value)?;
    if claim.lane != lane.claim_lane() {
        return Err(format!(
            "session start lane mismatch for {}; claim lane={} requested lane={}",
            issue.identifier,
            claim.lane.as_str(),
            lane.label()
        )
        .into());
    }
    if claim.issue != issue.identifier {
        return Err(format!(
            "session start issue mismatch for {}; claim points at {}",
            issue.identifier, claim.issue
        )
        .into());
    }
    if claim.run != run_id {
        return Err(format!(
            "session start run mismatch for {}; claim run={} requested run={run_id}",
            issue.identifier, claim.run
        )
        .into());
    }
    if claim.state != LaneClaimState::Active {
        return Err(format!(
            "session start requires an active claim; {} claim state={}",
            issue.identifier,
            claim.state.as_str()
        )
        .into());
    }
    if claim.worker.as_deref().unwrap_or("").trim().is_empty() {
        return Err(format!(
            "session start requires a structured worker= claim for {} run={}",
            issue.identifier, claim.run
        )
        .into());
    }
    Ok(claim)
}

fn agent_session_list(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    validate_tmux_session_config(&config)?;

    let output = ProcessCommand::new(&config.tmux.command)
        .args(["list-sessions", "-F", "#{session_name}:#{session_attached}"])
        .output();
    let Ok(output) = output else {
        println!("agent_session_list=unavailable reason=tmux_not_executable");
        return Ok(());
    };
    if !output.status.success() {
        println!("agent_session_list=none");
        return Ok(());
    }

    let prefix = format!("{}-", safe_identifier(&config.tmux.session_prefix));
    let mut found = false;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let (session, attached) = line.split_once(':').unwrap_or((line, "0"));
        if !session.starts_with(&prefix) {
            continue;
        }
        found = true;
        println!(
            "agent_session session={} attached={} attach_command=\"{} attach-session -t {}\"",
            session, attached, config.tmux.command, session
        );
    }
    if !found {
        println!("agent_session_list=none");
    }
    Ok(())
}

fn agent_session_attach(
    workflow_path: PathBuf,
    session: String,
    exec: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    validate_tmux_session_config(&config)?;

    let attach_command = format!("{} attach-session -t {}", config.tmux.command, session);
    println!("attach_command={attach_command}");
    if exec {
        let status = ProcessCommand::new(&config.tmux.command)
            .args(["attach-session", "-t", &session])
            .status()?;
        if !status.success() {
            return Err(format!(
                "tmux attach-session exited with status {}",
                status.code().unwrap_or(-1)
            )
            .into());
        }
    }
    Ok(())
}

fn validate_tmux_session_config(config: &RuntimeConfig) -> Result<(), Box<dyn std::error::Error>> {
    if config.tmux.command.trim().is_empty() {
        return Err("tmux.command must not be empty for session start".into());
    }
    if config.tmux.agent_command.trim().is_empty() {
        return Err("tmux.agent_command must not be empty for session start".into());
    }
    if config.tmux.session_prefix.trim().is_empty() {
        return Err("tmux.session_prefix must not be empty for session start".into());
    }
    Ok(())
}

fn agent_session_backend_spec(
    config: &RuntimeConfig,
    lane: AgentSessionLaneArg,
) -> Result<AgentSessionBackendSpec, Box<dyn std::error::Error>> {
    match lane {
        AgentSessionLaneArg::Main => main_agent_session_backend_spec(config, lane),
        AgentSessionLaneArg::Review => tmux_agent_session_backend_spec(config, lane),
        AgentSessionLaneArg::Merge => merge_agent_session_backend_spec(config, lane),
    }
}

fn main_agent_session_backend_spec(
    config: &RuntimeConfig,
    lane: AgentSessionLaneArg,
) -> Result<AgentSessionBackendSpec, Box<dyn std::error::Error>> {
    let backend = match config.backend.kind.as_str() {
        "codex" => "codex",
        "tmux" => "tmux",
        "claude-code" => "claude-code",
        "dry-run" => "dry-run",
        other => {
            return Err(format!(
                "unsupported main_lane.backend `{other}`; expected codex, tmux, claude-code, or dry-run"
            )
            .into())
        }
    };
    let command = match backend {
        "codex" => non_empty_session_command(
            &config.codex.command,
            "codex.command must not be empty for main session start",
        )?,
        "tmux" => {
            validate_tmux_session_config(config)?;
            tmux_agent_command_for_lane(config, lane)?
        }
        "claude-code" => non_empty_session_command(
            &config.claude.command,
            "claude.command must not be empty for main session start",
        )?,
        "dry-run" => "dry-run".into(),
        _ => unreachable!("validated main agent backend"),
    };

    Ok(AgentSessionBackendSpec {
        backend: backend.into(),
        command,
    })
}

fn tmux_agent_session_backend_spec(
    config: &RuntimeConfig,
    lane: AgentSessionLaneArg,
) -> Result<AgentSessionBackendSpec, Box<dyn std::error::Error>> {
    validate_tmux_session_config(config)?;
    Ok(AgentSessionBackendSpec {
        backend: "tmux".into(),
        command: tmux_agent_command_for_lane(config, lane)?,
    })
}

fn merge_agent_session_backend_spec(
    config: &RuntimeConfig,
    lane: AgentSessionLaneArg,
) -> Result<AgentSessionBackendSpec, Box<dyn std::error::Error>> {
    let requested = config.merge_lane.agent_backend.trim();
    let backend = match requested {
        "" | "codex" | "codex-app-server" | "app-server" => "codex",
        "tmux" => "tmux",
        "claude-code" => "claude-code",
        "dry-run" => "dry-run",
        other => {
            return Err(format!(
                "unsupported merge_lane.agent_backend `{other}`; expected codex, tmux, claude-code, or dry-run"
            )
            .into())
        }
    };
    let command = match backend {
        "codex" => non_empty_session_command(
            &config.codex.command,
            "codex.command must not be empty for merge session start",
        )?,
        "tmux" => {
            validate_tmux_session_config(config)?;
            tmux_agent_command_for_lane(config, lane)?
        }
        "claude-code" => non_empty_session_command(
            &config.claude.command,
            "claude.command must not be empty for merge session start",
        )?,
        "dry-run" => "dry-run".into(),
        _ => unreachable!("validated merge agent backend"),
    };

    Ok(AgentSessionBackendSpec {
        backend: backend.into(),
        command,
    })
}

fn non_empty_session_command(
    value: &str,
    message: &'static str,
) -> Result<String, Box<dyn std::error::Error>> {
    let command = value.trim();
    if command.is_empty() {
        Err(message.into())
    } else {
        Ok(command.to_string())
    }
}

fn agent_session_backend(
    backend: &str,
) -> Result<Box<dyn AgentBackend>, Box<dyn std::error::Error>> {
    match backend {
        "codex" => Ok(Box::<CodexBackend>::default()),
        "claude-code" => Ok(Box::<ClaudeCodeBackend>::default()),
        "tmux" => Ok(Box::<TmuxBackend>::default()),
        "dry-run" => Ok(Box::<DryRunBackend>::default()),
        other => Err(format!("unsupported agent session backend `{other}`").into()),
    }
}

fn tmux_agent_command_for_lane(
    config: &RuntimeConfig,
    lane: AgentSessionLaneArg,
) -> Result<String, Box<dyn std::error::Error>> {
    let command = match lane {
        AgentSessionLaneArg::Main => config
            .tmux
            .main_agent_command
            .as_deref()
            .unwrap_or(&config.tmux.agent_command),
        AgentSessionLaneArg::Review => config
            .tmux
            .review_agent_command
            .as_deref()
            .or_else(|| {
                (config.review.backend == "gemini-cli")
                    .then_some(config.review.gemini_command.as_str())
            })
            .unwrap_or(&config.tmux.agent_command),
        AgentSessionLaneArg::Merge => config
            .tmux
            .merge_agent_command
            .as_deref()
            .unwrap_or(&config.tmux.agent_command),
    };

    if command.trim().is_empty() {
        return Err(format!(
            "tmux {} agent command must not be empty for session start",
            lane.label()
        )
        .into());
    }

    Ok(command.to_string())
}

fn shell_quote_display(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.' | ':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn agent_session_workspace_key(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
) -> Result<String, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    let base = format!("{}-{}-agent", issue.identifier, lane.label());
    Ok(profile_scoped_identifier(
        profile
            .as_ref()
            .map(|profile| profile.workspace_namespace.as_str()),
        &base,
    ))
}

fn rendered_lane_prompt_artifact_path(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    attempt: u32,
    backend: &str,
) -> PathBuf {
    config.observability.logs_root.join("prompts").join(format!(
        "{}-{}-attempt-{}-{}-{}.prompt.md",
        safe_identifier(&issue.identifier),
        lane.label(),
        attempt,
        safe_identifier(backend),
        current_time_ms()
    ))
}

fn record_agent_session_events(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    summary: &jade_symphony::agent::AgentSummary,
    events: &[jade_symphony::model::AgentEvent],
    prompt_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    log.append(&EventRecord {
        event: "agent_session_prompt_artifact".into(),
        issue_id: Some(issue.id.clone()),
        issue_identifier: Some(issue.identifier.clone()),
        session_id: summary.session_id.clone(),
        profile_id: None,
        instance_name: None,
        actor_role: Some(config.identity.actor_role.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        git_author: config.identity.git.author(),
        tracker_mutation: None,
        message: format!(
            "lane={} prompt_artifact={}",
            lane.label(),
            prompt_path.display()
        ),
    })?;
    for event in events {
        log.append(&EventRecord {
            event: format!("agent_session_{event:?}"),
            issue_id: Some(issue.id.clone()),
            issue_identifier: Some(issue.identifier.clone()),
            session_id: summary.session_id.clone(),
            profile_id: None,
            instance_name: None,
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
            tracker_mutation: None,
            message: format!("lane={} {}", lane.label(), summary.message),
        })?;
    }
    Ok(())
}

struct AgentSessionWorkpadInput<'a> {
    issue: &'a TrackerIssue,
    lane: AgentSessionLaneArg,
    workspace_path: &'a Path,
    summary: &'a jade_symphony::agent::AgentSummary,
    prompt_path: &'a Path,
    claim_value: &'a str,
    agent_command: &'a str,
    git_identity: &'a GitIdentityApplyResult,
}

fn agent_session_workpad(input: AgentSessionWorkpadInput<'_>) -> String {
    let attach_command = input.summary.attach_command.as_deref().unwrap_or("n/a");
    let log_path = input
        .summary
        .log_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "n/a".into());
    let title = match input.lane {
        AgentSessionLaneArg::Main => "## Jade Symphony Workpad",
        AgentSessionLaneArg::Review => "## Jade Symphony Agent Review Run",
        AgentSessionLaneArg::Merge => "## Jade Symphony Merge Run",
    };
    let session_heading = if input.summary.backend == "tmux" {
        "### Local tmux Agent Session"
    } else {
        "### Local Agent Session"
    };
    let evidence_summary = if input.summary.backend == "tmux" {
        "tmux session, prompt artifact, log path, workspace, and claim metadata recorded."
    } else {
        "backend session, prompt artifact, workspace, and claim metadata recorded."
    };
    [
        title.to_string(),
        String::new(),
        session_heading.to_string(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", input.issue.identifier, input.issue.title),
        format!("- Lane: `{}`", input.lane.label()),
        format!(
            "- Actor role: `{}`",
            match input.lane {
                AgentSessionLaneArg::Main => "implementation_agent",
                AgentSessionLaneArg::Review => "review_agent",
                AgentSessionLaneArg::Merge => "merge_agent",
            }
        ),
        format!(
            "- Actor: `{}`",
            timeline_claim_actor(input.claim_value).unwrap_or_else(|| "not recorded".into())
        ),
        format!(
            "- Run ID: `{}`",
            timeline_claim_run(input.claim_value).unwrap_or_else(|| "not recorded".into())
        ),
        format!(
            "- Input state: `{}`",
            match input.lane {
                AgentSessionLaneArg::Main => input.issue.state.as_str(),
                AgentSessionLaneArg::Review => "Agent Review",
                AgentSessionLaneArg::Merge => "Merging",
            }
        ),
        format!(
            "- Target state after run: `{}`",
            match input.lane {
                AgentSessionLaneArg::Main => "Agent Review",
                AgentSessionLaneArg::Review => {
                    "Human Review | Merging | Rework | Need Human Input | unchanged"
                }
                AgentSessionLaneArg::Merge => "Done | Need Human Input | unchanged",
            }
        ),
        format!(
            "- Result: `{}`",
            if input.summary.pending_session {
                "session_started"
            } else {
                "session_recorded"
            }
        ),
        format!("- PR: `{}`", timeline_pr_summary(input.issue)),
        format!(
            "- Claim field: `{}` = `{}`",
            input.lane.claim_field(),
            input.claim_value
        ),
        format!("- Backend: `{}`", input.summary.backend),
        format!("- Agent command: `{}`", input.agent_command),
        format!(
            "- Session: `{}`",
            input.summary.session_id.as_deref().unwrap_or("n/a")
        ),
        format!("- Pending session: `{}`", input.summary.pending_session),
        format!("- Workspace: `{}`", input.workspace_path.display()),
        format!("- Prompt artifact: `{}`", input.prompt_path.display()),
        format!("- Session log: `{log_path}`"),
        format!("- Attach command: `{attach_command}`"),
        format!("- Git identity: `{}`", input.git_identity.summary()),
        format!("- Evidence summary: {evidence_summary}"),
        String::new(),
        input.summary.message.clone(),
    ]
    .join("\n")
}

fn apply_review_result(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    issue: &TrackerIssue,
    job: &jade_symphony::review::ReviewJob,
    claim: Option<&LaneClaim>,
    repeat_evidence: Option<&ReviewRepeatedFailureEvidence>,
) -> Result<(), Box<dyn std::error::Error>> {
    let decision = review_gate_decision_for_issue(job, issue);
    if let Some(value) = terminal_review_loop_claim_value(claim, job, &decision) {
        write_terminal_review_claim(
            config,
            adapter,
            issue_ref,
            &issue.state,
            &value,
            "review loop terminal claim evidence",
        )?;
    }
    if decision.outcome.is_passed() {
        update_review_checklist_for_pass(
            config,
            adapter,
            issue,
            decision.target_state.unwrap_or("none"),
        )?;
    }
    if let Some(target_state) = decision.target_state {
        if !transition_allowed_for_review_agent(target_state, &decision) {
            return Err("review agent transition is not allowed for this review decision".into());
        }
        if rework_transition_expected(&decision) {
            transition_review_issue_to_rework_with_workpad(config, adapter, issue, job)?;
            return Ok(());
        }
    }

    let workpad = repeat_evidence
        .map(|evidence| render_repeated_review_failure_workpad(issue, job, evidence))
        .unwrap_or_else(|| render_review_workpad(issue, job));
    let evidence_key = recovery_key(
        "review-result",
        issue_ref,
        &format!(
            "{}|{:?}|{}|{}",
            issue_ref,
            decision.outcome,
            decision.target_state.unwrap_or("none"),
            job.ledger_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| stable_recovery_hash(&workpad))
        ),
    );
    let evidence_outcome = add_timeline_comment_with_recovery(
        adapter,
        issue_ref,
        Some(issue),
        &workpad,
        &evidence_key,
        "timeline_comment",
    )?;
    if evidence_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "review loop",
                mutation_type: "timeline_comment",
                issue_ref: Some(issue_ref),
                target: job
                    .ledger_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                from_state: Some(issue.state.clone()),
                to_state: decision.target_state.map(ToOwned::to_owned),
                reason: "review result timeline evidence",
            },
        );
    }
    if let Some(target_state) = decision.target_state {
        let state_outcome = set_state_with_recovery(
            adapter,
            issue_ref,
            Some(issue),
            target_state,
            "state_change",
        )?;
        if state_outcome.should_record_audit() {
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "review loop",
                    mutation_type: "state_change",
                    issue_ref: Some(issue_ref),
                    target: None,
                    from_state: Some(issue.state.clone()),
                    to_state: Some(target_state.into()),
                    reason: "review result routing",
                },
            );
        }
    }
    Ok(())
}

fn update_review_checklist_for_pass(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    target_state: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(description) = issue.description.as_deref() else {
        return Ok(());
    };
    let body = canonical_issue_body_without_workpad(description);
    let updated = check_review_verified_issue_body_checkboxes(&body);
    if updated == body {
        return Ok(());
    }

    adapter.update_issue_content(&issue.identifier, &issue.title, &updated)?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "review loop",
            mutation_type: "issue_body_update",
            issue_ref: Some(&issue.identifier),
            target: Some("non-UAT review checkboxes".into()),
            from_state: Some(issue.state.clone()),
            to_state: Some(target_state.into()),
            reason: "automatic review pass checklist evidence",
        },
    );
    Ok(())
}

fn canonical_issue_body_without_workpad(description: &str) -> String {
    description
        .split("<!-- jade-symphony-workpad -->")
        .next()
        .unwrap_or(description)
        .trim_end()
        .to_string()
}

fn check_review_verified_issue_body_checkboxes(body: &str) -> String {
    let mut in_fence = false;
    let mut in_review_section = false;
    let mut lines = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            lines.push(line.to_string());
            continue;
        }
        if !in_fence {
            if let Some(section) = markdown_heading_title(trimmed) {
                in_review_section = review_checklist_section_is_agent_owned(section);
            }
        }
        if in_review_section && !in_fence {
            lines.push(check_markdown_checkbox_line(line));
        } else {
            lines.push(line.to_string());
        }
    }
    let mut updated = lines.join("\n");
    if body.ends_with('\n') {
        updated.push('\n');
    }
    updated
}

fn markdown_heading_title(line: &str) -> Option<&str> {
    let heading_len = line.chars().take_while(|ch| *ch == '#').count();
    if heading_len == 0 || heading_len > 6 {
        return None;
    }
    if !line
        .chars()
        .nth(heading_len)
        .is_some_and(char::is_whitespace)
    {
        return None;
    }
    Some(line[heading_len..].trim().trim_matches('#').trim())
}

fn review_checklist_section_is_agent_owned(section: &str) -> bool {
    matches!(
        section.to_ascii_lowercase().as_str(),
        "expected outcome"
            | "completion criteria"
            | "functional verification"
            | "context verification"
    )
}

fn check_markdown_checkbox_line(line: &str) -> String {
    let trimmed = line.trim_start();
    if !(trimmed.starts_with("- [ ]") || trimmed.starts_with("* [ ]")) {
        return line.to_string();
    }
    if let Some(index) = line.find("[ ]") {
        let mut checked = line.to_string();
        checked.replace_range(index..index + 3, "[x]");
        checked
    } else {
        line.to_string()
    }
}

fn terminal_review_loop_claim_value(
    claim: Option<&LaneClaim>,
    job: &jade_symphony::review::ReviewJob,
    decision: &ReviewGateDecision,
) -> Option<String> {
    let claim = claim?;
    let (state, result) = match decision.outcome {
        ReviewOutcome::PassedToHumanReview | ReviewOutcome::PassedToMerging => {
            (LaneClaimState::Done, "passed")
        }
        ReviewOutcome::NeedsRework => (LaneClaimState::Done, "rejected"),
        ReviewOutcome::InconclusiveNeedsRework => (LaneClaimState::Failed, "inconclusive"),
        ReviewOutcome::NeedsHumanInput => (LaneClaimState::Failed, "blocked"),
        ReviewOutcome::BackendUnavailable => (LaneClaimState::Failed, "unavailable"),
        ReviewOutcome::Cancelled => (LaneClaimState::Failed, "cancelled"),
        ReviewOutcome::StillRunning => match job.state {
            ReviewJobState::Failed | ReviewJobState::TimedOut => {
                (LaneClaimState::Failed, "unavailable")
            }
            ReviewJobState::Cancelled => (LaneClaimState::Failed, "cancelled"),
            ReviewJobState::Queued | ReviewJobState::Running | ReviewJobState::Completed => {
                return None;
            }
        },
    };
    Some(terminal_review_claim_value(claim, state, result))
}

#[cfg(test)]
fn transition_issue_to_rework_with_diagnostic(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    diagnostic: &ReworkDiagnostic,
) -> Result<(), Box<dyn std::error::Error>> {
    let workpad = render_rework_diagnostic_workpad(issue, diagnostic);
    adapter.add_issue_comment(&issue.identifier, &workpad)?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "review loop",
            mutation_type: "timeline_comment",
            issue_ref: Some(&issue.identifier),
            target: diagnostic.review_ledger_path.clone(),
            from_state: Some(issue.state.clone()),
            to_state: Some("rework".into()),
            reason: "review rework diagnostic",
        },
    );
    adapter.set_state(&issue.identifier, "rework")?;
    append_tracker_mutation_audit(
        config,
        TrackerMutationAudit {
            command: "review loop",
            mutation_type: "state_change",
            issue_ref: Some(&issue.identifier),
            target: None,
            from_state: Some(issue.state.clone()),
            to_state: Some("rework".into()),
            reason: "confirmed review finding",
        },
    );
    Ok(())
}

fn transition_review_issue_to_rework_with_workpad(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    job: &ReviewJob,
) -> Result<(), Box<dyn std::error::Error>> {
    let workpad = render_review_workpad(issue, job);
    let evidence_key = recovery_key(
        "review-rework",
        &issue.identifier,
        &format!(
            "{}|{}",
            issue.identifier,
            job.ledger_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| stable_recovery_hash(&workpad))
        ),
    );
    let evidence_outcome = add_timeline_comment_with_recovery(
        adapter,
        &issue.identifier,
        Some(issue),
        &workpad,
        &evidence_key,
        "timeline_comment",
    )?;
    if evidence_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "review loop",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue.identifier),
                target: job
                    .ledger_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                from_state: Some(issue.state.clone()),
                to_state: Some("rework".into()),
                reason: "review result timeline evidence",
            },
        );
    }
    let state_outcome = set_state_with_recovery(
        adapter,
        &issue.identifier,
        Some(issue),
        "rework",
        "state_change",
    )?;
    if state_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "review loop",
                mutation_type: "state_change",
                issue_ref: Some(&issue.identifier),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: Some("rework".into()),
                reason: "confirmed review finding",
            },
        );
    }
    Ok(())
}

fn require_write_intent(write: bool) -> Result<(), Box<dyn std::error::Error>> {
    if write {
        Ok(())
    } else {
        Err("live write command requires explicit --write".into())
    }
}

fn warn_if_temporary_workflow_path(workflow_path: &Path) {
    if let Some(warning) = temporary_workflow_warning(workflow_path) {
        eprintln!("{warning}");
    }
}

fn temporary_workflow_warning(workflow_path: &Path) -> Option<String> {
    if !is_temporary_workflow_path(workflow_path) {
        return None;
    }
    Some(format!(
        "workflow_warning=temporary_path path={} action=promote durable_config=examples/ docs=docs/operator-dogfood.md",
        workflow_path.display()
    ))
}

fn is_temporary_workflow_path(workflow_path: &Path) -> bool {
    [Path::new("/private/tmp"), Path::new("/tmp")]
        .iter()
        .any(|prefix| workflow_path.starts_with(prefix))
        || workflow_path.starts_with(std::env::temp_dir())
}

fn load_config(workflow_path: &Path) -> Result<RuntimeConfig, Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(workflow_path);
    let workflow = WorkflowDefinition::load(workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, workflow_path)?;
    config.validate()?;
    Ok(config)
}

fn progress_spec_for_config(config: &RuntimeConfig, wait: &str) -> ProgressHeartbeatSpec {
    ProgressHeartbeatSpec::new(wait).actor(
        config.identity.actor_role.clone(),
        config.identity.actor_label.clone(),
    )
}

fn progress_spec_with_event_log(config: &RuntimeConfig, wait: &str) -> ProgressHeartbeatSpec {
    progress_spec_for_config(config, wait)
        .event_log_path(config.observability.logs_root.join("jade-symphony.jsonl"))
}

fn tracker_backend_label(config: &RuntimeConfig) -> &'static str {
    match config.tracker.kind.as_str() {
        "github_project_v2" => "gh",
        "linear" => "linear",
        "memory" => "memory",
        _ => "tracker",
    }
}

fn validate(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(&workflow_path);
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    println!("workflow={}", workflow_path.display());
    println!("tracker={}", config.tracker.kind);
    println!("backend={}", config.backend.kind);
    println!("workspace_root={}", config.workspace.root.display());
    println!("prompt_template_bytes={}", workflow.prompt_template.len());
    println!("status=valid");
    Ok(())
}

fn inspect(
    workflow_path: PathBuf,
    state_filters: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .backend(tracker_backend_label(&config))
            .next("load_project_summary"),
        || adapter.list_project_summary_issues(),
    )?;
    let issues = filter_issues_by_state(issues, &state_filters);

    if !state_filters.is_empty() {
        println!("state_filter={}", state_filters.join(","));
    }
    println!("issues={}", issues.len());
    println!("{}", render_state_summary(&issues));
    for issue in issues {
        let gate = evaluate_issue_for_current_source(&config, &issue)?;
        println!(
            "- {} {} state={} gate={:?}",
            issue.identifier, issue.title, issue.state, gate.kind
        );
        if !gate.missing.is_empty() {
            println!("  missing={}", gate.missing.join(", "));
        }
        if !gate.assumptions.is_empty() {
            println!("  assumptions={}", gate.assumptions.join("; "));
        }
    }

    for gap in adapter.integration_gaps() {
        println!("integration_gap={gap}");
    }

    Ok(())
}

fn project_state(options: ProjectStateOptions) -> Result<(), Box<dyn std::error::Error>> {
    let workflow_path = options.workflow_path;
    warn_if_temporary_workflow_path(&workflow_path);
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let read_result = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .backend(tracker_backend_label(&config))
            .next("load_project_summary"),
        || adapter.list_project_summary_issues(),
    );
    match read_result {
        Ok(issues) => {
            let mut integration_gaps = adapter.integration_gaps();
            append_canonical_checkout_gap(&config, &mut integration_gaps);
            if options.display == DisplayMode::Tui {
                println!("{}", render_project_state_panel(&issues, &integration_gaps));
                return Ok(());
            }
            println!("project_state_access=ok");
            println!("trusted=true");
            println!("issues={}", issues.len());
            println!("empty_queue={}", issues.is_empty());
            println!("{}", render_state_summary(&issues));
            for line in report_canonical_checkout_readonly(&config) {
                println!("{line}");
            }
            for gap in integration_gaps {
                println!("integration_gap={gap}");
            }
            Ok(())
        }
        Err(error) => {
            let kind = classify_project_state_error(&error);
            println!("project_state_access=blocked");
            println!("trusted=false");
            println!("failure_kind={}", kind.as_str());
            println!("failure={error}");
            Err(format!(
                "project state access is not trustworthy: kind={} error={error}",
                kind.as_str()
            )
            .into())
        }
    }
}

fn project_issue(
    workflow_path: PathBuf,
    issue_ref: String,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .issue(issue_ref.clone())
            .backend(tracker_backend_label(&config))
            .next("load_issue"),
        || adapter.get_issue(&issue_ref),
    )?
    .ok_or_else(|| format!("issue not found: {issue_ref}"))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&issue)?);
        return Ok(());
    }

    println!("issue={}", issue.identifier);
    println!("title={}", issue.title);
    println!("state={}", issue.state);
    println!("tracker={}", issue.tracker_kind);
    if let Some(item_id) = &issue.item_id {
        println!("project_item={item_id}");
    }
    if !issue.assignees.is_empty() {
        println!("assignees={}", issue.assignees.join(","));
    }
    if !issue.blocked_by.is_empty() {
        let blockers = issue
            .blocked_by
            .iter()
            .map(|blocker| {
                blocker
                    .identifier
                    .as_deref()
                    .or(blocker.id.as_deref())
                    .unwrap_or("unknown")
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(",");
        println!("blocked_by={blockers}");
    } else {
        println!("blocked_by=");
    }
    if !issue.linked_pull_requests.is_empty() {
        for pr in &issue.linked_pull_requests {
            let pr_ref = pr
                .url
                .clone()
                .or_else(|| pr.number.map(|number| format!("#{number}")))
                .unwrap_or_else(|| "unknown".into());
            println!(
                "linked_pr={} state={}",
                pr_ref,
                pr.state.as_deref().unwrap_or("unknown")
            );
        }
    }
    for (name, value) in &issue.project_fields {
        println!("field.{name}={}", compact_json_value(value));
    }
    Ok(())
}

fn hydrate_issue_for_evidence(
    adapter: &dyn TrackerAdapter,
    issue: TrackerIssue,
    project_context: &[TrackerIssue],
) -> Result<TrackerIssue, TrackerError> {
    adapter.hydrate_issue_evidence(issue, project_context)
}

fn hydrate_issues_for_review_lane(
    adapter: &dyn TrackerAdapter,
    issues: Vec<TrackerIssue>,
) -> Result<Vec<TrackerIssue>, TrackerError> {
    let project_context = issues.clone();
    issues
        .into_iter()
        .map(|issue| hydrate_issue_for_evidence(adapter, issue, &project_context))
        .collect()
}

fn hydrate_issues_for_doctor(
    adapter: &dyn TrackerAdapter,
    issues: Vec<TrackerIssue>,
) -> Result<Vec<TrackerIssue>, TrackerError> {
    let project_context = issues.clone();
    let rich_refs = doctor_issue_refs_requiring_rich_hydration(&issues);
    issues
        .into_iter()
        .map(|issue| {
            if rich_refs.contains(&issue.identifier) {
                hydrate_issue_for_evidence(adapter, issue, &project_context)
            } else {
                Ok(issue)
            }
        })
        .collect()
}

fn doctor_issue_refs_requiring_rich_hydration(issues: &[TrackerIssue]) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    for issue in issues {
        let topology_needs_rich = doctor_issue_topology_needs_rich_hydration(issue);
        if doctor_issue_state_needs_rich_hydration(issue) || topology_needs_rich {
            refs.insert(issue.identifier.clone());
        }
        if topology_needs_rich {
            for subissue in native_subissue_statuses(issue) {
                refs.insert(subissue.identifier);
            }
        }
    }
    refs
}

fn doctor_issue_state_needs_rich_hydration(issue: &TrackerIssue) -> bool {
    matches!(
        issue.normalized_state().as_str(),
        "todo" | "need to clarify" | "in progress" | "agent review" | "human review" | "merging"
    )
}

fn doctor_issue_topology_needs_rich_hydration(issue: &TrackerIssue) -> bool {
    if !issue_has_native_topology(issue) {
        return false;
    }
    matches!(
        issue.normalized_state().as_str(),
        "todo" | "in progress" | "agent review" | "human review" | "rework" | "merging"
    )
}

fn issue_has_native_topology(issue: &TrackerIssue) -> bool {
    issue.project_fields.contains_key("GitHub Native Parent")
        || issue.project_fields.contains_key("Native Parent Issue")
        || issue.project_fields.contains_key("GitHub Native Subissues")
        || issue.project_fields.contains_key("Native Subissues")
}

fn project_inspect(
    workflow_path: PathBuf,
    issue_ref: String,
    lane: Option<AgentSessionLaneArg>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let gate = evaluate_issue_for_current_source(&config, &issue)?;
    let terminal_states = config.terminal_state_set().into_iter().collect();
    let native_subissue_blocker = native_subissue_gate_blocker(&issue, &terminal_states);

    println!("project_inspect=ok");
    println!("read_only=true");
    println!("issue={}", issue.identifier);
    println!("title={}", issue.title);
    println!("state={}", issue.state);
    if let Some(lane) = lane {
        println!("lane={}", lane.label());
    }
    println!("gate={:?}", gate.kind);
    println!(
        "dispatchable={}",
        gate.is_dispatchable() && native_subissue_blocker.is_none()
    );
    if let Some(reason) = &native_subissue_blocker {
        println!("native_subissue_gate={reason}");
    }
    if !gate.missing.is_empty() {
        println!("missing={}", gate.missing.join(", "));
    }
    if !gate.assumptions.is_empty() {
        println!("assumptions={}", gate.assumptions.join("; "));
    }
    if issue.blocked_by.is_empty() {
        println!("blocked_by=");
    } else {
        let blockers = issue
            .blocked_by
            .iter()
            .map(|blocker| {
                blocker
                    .identifier
                    .as_deref()
                    .or(blocker.id.as_deref())
                    .unwrap_or("unknown")
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(",");
        println!("blocked_by={blockers}");
    }
    if issue.linked_pull_requests.is_empty() {
        println!("linked_prs=");
    } else {
        for pr in &issue.linked_pull_requests {
            let pr_ref = pr
                .url
                .clone()
                .or_else(|| pr.number.map(|number| format!("#{number}")))
                .unwrap_or_else(|| "unknown".into());
            println!(
                "linked_pr={} state={}",
                pr_ref,
                pr.state.as_deref().unwrap_or("unknown")
            );
        }
    }
    let review_status_options = ReviewStatusOptions {
        issue_filter: Some(issue.identifier.clone()),
        recent_limit: 1,
        verbose: false,
    };
    if let Ok(payload) = load_review_status(
        &config,
        std::slice::from_ref(&issue),
        &review_status_options,
        unix_timestamp_ms(),
    ) {
        if let Some(summary) = render_project_inspect_review_summary(&payload) {
            println!("review_status_summary={summary}");
        }
    }
    for gap in adapter.integration_gaps() {
        println!("integration_gap={gap}");
    }

    Ok(())
}

fn compact_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Null => "null".into(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "<unprintable>".into()),
    }
}

fn filter_issues_by_state(
    issues: Vec<TrackerIssue>,
    state_filters: &[String],
) -> Vec<TrackerIssue> {
    if state_filters.is_empty() {
        return issues;
    }

    let normalized_filters = state_filters
        .iter()
        .map(|state| normalize_state(state))
        .collect::<Vec<_>>();
    issues
        .into_iter()
        .filter(|issue| {
            let issue_state = issue.normalized_state();
            normalized_filters
                .iter()
                .any(|filter| filter == &issue_state)
        })
        .collect()
}

fn render_state_summary(issues: &[TrackerIssue]) -> String {
    let mut counts = BTreeMap::new();
    for issue in issues {
        let state = issue.state.trim();
        let state = if state.is_empty() { "(unknown)" } else { state };
        *counts.entry(state.to_string()).or_insert(0usize) += 1;
    }

    let summary = if counts.is_empty() {
        "(none)".to_string()
    } else {
        counts
            .into_iter()
            .map(|(state, count)| format!("{state}:{count}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    format!("state_summary={summary}")
}

fn doctor(options: DoctorOptions) -> Result<(), Box<dyn std::error::Error>> {
    let workflow_path = resolve_doctor_workflow_path(options.workflow_path.clone());
    if options.json && options.display == DisplayMode::Tui {
        return Err("doctor --json cannot be combined with --display tui".into());
    }
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .backend(tracker_backend_label(&config))
            .next("doctor_project_scan"),
        || adapter.fetch_issues_by_states(&all_mapped_tracker_states(&config)),
    )?;
    let issues = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .backend(tracker_backend_label(&config))
            .next("doctor_hydrate_issues"),
        || hydrate_issues_for_doctor(adapter.as_ref(), issues),
    )?;
    let mut integration_gaps = adapter.integration_gaps();
    append_canonical_checkout_gap(&config, &mut integration_gaps);
    let runtime_states = match load_runtime_states(&config) {
        Ok(states) => states,
        Err(error) => {
            integration_gaps.push(format!("runtime_state_load_error: {error}"));
            Vec::new()
        }
    };
    let sessions = match session_status_snapshots(&config) {
        Ok(sessions) => sessions,
        Err(error) => {
            integration_gaps.push(format!("tmux_session_status_unavailable: {error}"));
            Vec::new()
        }
    };
    let context = ProjectDoctorContext {
        runtime_state: runtime_states.first().cloned(),
        runtime_states,
        sessions,
        now_ms: current_time_ms(),
        stale_after_ms: options.stale_after_ms,
    };
    let mut report = audit_project_issues_with_context(&issues, Some(&context));
    report.integration_gaps = integration_gaps;
    append_canonical_checkout_doctor_violations(&mut report, &config);
    append_workspace_doctor_violations(&mut report, &config, &issues);
    let skill_repo_root = discover_skill_suite_repo_root(&workflow_path)?;
    let skill_targets = default_jade_symphony_skill_targets();
    append_local_skill_install_doctor_violations(&mut report, &skill_repo_root, &skill_targets);
    report.skill_readiness_summary = Some(doctor_skill_readiness_summary(SkillStatusInput {
        workflow_path: workflow_path.clone(),
        suite_path: None,
        codex_dir: None,
        gemini_dir: None,
        require_gemini: false,
        session_skills: Vec::new(),
        session_skills_file: None,
    }));

    match &options.action {
        Some(DoctorAction::Repair(repair)) => {
            doctor_repair_issue(&config, adapter.as_ref(), &issues, &report, repair)?;
            return Ok(());
        }
        None if options.json => {
            println!("{}", render_project_audit_report_json(&report)?);
        }
        None => {
            if options.display == DisplayMode::Tui {
                println!("{}", render_doctor_panel(&report));
            } else {
                println!("{}", render_project_audit_report(&report));
            }
            if options.interactive {
                print_doctor_interactive_plan(&report);
            }
            if options.auto_fix {
                apply_doctor_auto_fix(&config, adapter.as_ref(), &report, options.write)?;
            }
        }
    }

    if options.strict && report.blocker_count() > 0 {
        return Err(format!(
            "project doctor strict mode found {} blocker violation(s)",
            report.blocker_count()
        )
        .into());
    }

    Ok(())
}

fn skills_status(input: SkillStatusInput, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let report = build_skill_readiness_report(input);
    if json {
        println!("{}", render_skill_readiness_report_json(&report)?);
    } else {
        println!("{}", render_skill_readiness_report(&report));
    }
    Ok(())
}

fn resolve_doctor_workflow_path(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }
    if let Some(path) = std::env::var_os("JADE_SYMPHONY_WORKFLOW")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return path;
    }
    let repo_default = PathBuf::from("workflows/jade-symphony.md");
    if repo_default.exists() {
        repo_default
    } else {
        PathBuf::from("WORKFLOW.md")
    }
}

fn discover_skill_suite_repo_root(
    workflow_path: &Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let start = if workflow_path.is_absolute() {
        workflow_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(workflow_path)
    };
    let mut cursor = start
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    loop {
        if cursor
            .join("skills")
            .join("jade-symphony")
            .join("manifest.toml")
            .exists()
        {
            return Ok(cursor);
        }
        if !cursor.pop() {
            break;
        }
    }
    Ok(std::env::current_dir()?)
}

fn print_doctor_interactive_plan(report: &ProjectAuditReport) {
    println!(
        "doctor_interactive findings={} blockers={}",
        report.violations.len(),
        report.blocker_count()
    );
    if report.violations.is_empty() {
        println!("doctor_interactive action=no_op reason=no_fixable_findings");
        return;
    }
    for violation in &report.violations {
        let command = if violation.code == AGENT_REVIEW_DRAFT_PR {
            format!(
                "doctor repair {} --mark-pr-ready --confirm-handoff-ready --write",
                violation.issue_ref.trim_start_matches('#')
            )
        } else {
            format!(
                "doctor repair {}",
                violation.issue_ref.trim_start_matches('#')
            )
        };
        println!(
            "doctor_interactive action=inspect issue={} code={} command=\"{}\"",
            violation.issue_ref, violation.code, command
        );
    }
}

fn apply_doctor_auto_fix(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    report: &ProjectAuditReport,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let candidates = human_review_repair_candidates(report);
    let draft_pr_candidates = draft_pr_repair_candidates(report);
    println!(
        "doctor_auto_fix safe_candidates={} write={write}",
        candidates.len()
    );
    for violation in draft_pr_candidates {
        println!(
            "doctor_auto_fix action=skip issue={} code={} reason=pr_ready_requires_operator_confirmation",
            violation.issue_ref, violation.code
        );
    }
    for violation in candidates {
        println!(
            "doctor_auto_fix action=move issue={} from={:?} to=agent_review",
            violation.issue_ref, violation.state
        );
        if write {
            let workpad = render_human_review_repair_workpad(violation);
            adapter.add_issue_comment(&violation.issue_ref, &workpad)?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor --auto-fix",
                    mutation_type: "timeline_comment",
                    issue_ref: Some(&violation.issue_ref),
                    target: None,
                    from_state: Some(violation.state.clone()),
                    to_state: Some("agent_review".into()),
                    reason: "doctor auto-fix evidence",
                },
            );
            adapter.set_state(&violation.issue_ref, "agent_review")?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor --auto-fix",
                    mutation_type: "state_change",
                    issue_ref: Some(&violation.issue_ref),
                    target: None,
                    from_state: Some(violation.state.clone()),
                    to_state: Some("agent_review".into()),
                    reason: "safe doctor auto-fix for invalid Human Review boundary",
                },
            );
        } else {
            println!(
                "doctor_auto_fix_dry_run action=timeline_comment issue={} evidence=human_review_missing_review_evidence",
                violation.issue_ref
            );
            println!(
                "doctor_auto_fix_dry_run action=set_state issue={} target_state=agent_review",
                violation.issue_ref
            );
        }
    }
    Ok(())
}

fn append_canonical_checkout_doctor_violations(
    report: &mut ProjectAuditReport,
    config: &RuntimeConfig,
) {
    let root = match std::env::current_dir() {
        Ok(root) => root,
        Err(error) => {
            report
                .integration_gaps
                .push(format!("canonical_checkout_unavailable: {error}"));
            return;
        }
    };
    let checkout = match inspect_canonical_checkout(&root, config) {
        Ok(report) => report,
        Err(error) => {
            report
                .integration_gaps
                .push(format!("canonical_checkout_unavailable: {error}"));
            return;
        }
    };
    report
        .integration_gaps
        .push(canonical_checkout_status_line(&checkout));

    if !checkout.tracked_dirty.is_empty() {
        report.violations.push(ProjectAuditViolation {
            issue_ref: "canonical".into(),
            title: "Canonical checkout has tracked dirty files".into(),
            state: "local".into(),
            severity: AuditSeverity::Blocker,
            code: "canonical_checkout_tracked_dirty".into(),
            message: format!(
                "Canonical checkout has tracked dirty files: {}",
                checkout.tracked_dirty.join(", ")
            ),
            suggestion: "Move the edits into the correct issue worktree, commit them, or restore them before running any live write lane.".into(),
        });
    }

    let unclassified = checkout.unclassified_untracked();
    if !unclassified.is_empty() {
        report.violations.push(ProjectAuditViolation {
            issue_ref: "canonical".into(),
            title: "Canonical checkout has unclassified untracked files".into(),
            state: "local".into(),
            severity: AuditSeverity::Warning,
            code: "canonical_checkout_unclassified_untracked".into(),
            message: format!(
                "Canonical checkout has unclassified untracked files: {}",
                unclassified
                    .iter()
                    .map(|entry| entry.path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            suggestion: "Move unclassified files to an issue worktree or artifact location, or add legitimate ignored files to .gitignore.".into(),
        });
    }
}

fn append_workspace_doctor_violations(
    report: &mut ProjectAuditReport,
    config: &RuntimeConfig,
    issues: &[TrackerIssue],
) {
    let registry = match load_session_registry(&session_registry_path(config)) {
        Ok(registry) => registry,
        Err(error) => {
            report
                .integration_gaps
                .push(format!("workspace_session_registry_unavailable: {error}"));
            return;
        }
    };
    let worktrees = match std::env::current_dir()
        .ok()
        .and_then(|cwd| git_worktree_list(&cwd).ok())
    {
        Some(worktrees) => worktrees,
        None => {
            report
                .integration_gaps
                .push("workspace_git_worktree_scan_unavailable".into());
            return;
        }
    };

    for issue in issues {
        if !matches!(
            issue.normalized_state().as_str(),
            "in progress" | "agent review" | "rework" | "merging"
        ) {
            continue;
        }
        let workspace_report = discover_issue_workspaces_from_parts(
            issue,
            &registry.sessions,
            &worktrees,
            &config.tracker.workpad.marker,
        );
        if workspace_report
            .warnings
            .iter()
            .any(|warning| warning.contains("multiple strong"))
        {
            report.violations.push(ProjectAuditViolation {
                issue_ref: issue.identifier.clone(),
                title: issue.title.clone(),
                state: issue.state.clone(),
                severity: AuditSeverity::Warning,
                code: "workspace_ambiguous_candidates".into(),
                message: format!(
                    "Issue has {} strong workspace candidates.",
                    workspace_report
                        .candidates
                        .iter()
                        .filter(|candidate| {
                            candidate.strength
                                == jade_symphony::issue_workspace::WorkspaceMatchStrength::Strong
                        })
                        .count()
                ),
                suggestion: "Run `workspace show <workflow> <issue>` and then `workspace adopt <workflow> <issue> <path> --write` before lane repair uses a worktree.".into(),
            });
        }
    }
}

fn doctor_repair_issue(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issues: &[TrackerIssue],
    report: &ProjectAuditReport,
    repair: &DoctorRepairIssueOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let issue = issues
        .iter()
        .find(|issue| issue_ref_matches(&issue.identifier, &repair.issue_ref))
        .ok_or_else(|| format!("doctor repair could not find issue {}", repair.issue_ref))?;
    println!(
        "doctor_repair issue={} state={:?} write={} move_need_human_input={} mark_pr_ready={} confirm_handoff_ready={}",
        issue.identifier,
        issue.state,
        repair.write,
        repair.move_need_human_input,
        repair.mark_pr_ready,
        repair.confirm_handoff_ready
    );
    println!(
        "safe=no_op command=\"doctor repair {}\"",
        issue.identifier.trim_start_matches('#')
    );
    println!("uncertain=resume command=\"main loop <workflow> --write\" reason=requires operator confirmation and live workspace inspection");
    println!("uncertain=reset reason=requires confirming no useful work would be discarded");
    println!("uncertain=move_need_human_input command=\"doctor repair {} --move-need-human-input --write\" reason=records evidence before tracker mutation", issue.identifier.trim_start_matches('#'));
    println!("uncertain=mark_pr_ready command=\"doctor repair {} --mark-pr-ready --confirm-handoff-ready --write\" reason=requires operator-confirmed handoff evidence", issue.identifier.trim_start_matches('#'));
    println!("dangerous=delete_worktree reason=out_of_scope_for_doctor_repair");

    if repair.move_need_human_input {
        let workpad = render_doctor_repair_workpad(issue, report, "move_need_human_input");
        if repair.write {
            adapter.add_issue_comment(&issue.identifier, &workpad)?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor repair",
                    mutation_type: "timeline_comment",
                    issue_ref: Some(&issue.identifier),
                    target: None,
                    from_state: Some(issue.state.clone()),
                    to_state: Some("need_human_input".into()),
                    reason: "doctor repair evidence before human-input escalation",
                },
            );
            adapter.set_state(&issue.identifier, "need_human_input")?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor repair",
                    mutation_type: "state_change",
                    issue_ref: Some(&issue.identifier),
                    target: None,
                    from_state: Some(issue.state.clone()),
                    to_state: Some("need_human_input".into()),
                    reason: "doctor repair escalated uncertain runtime state",
                },
            );
        } else {
            println!(
                "doctor_repair_dry_run action=timeline_comment issue={} evidence=doctor_repair",
                issue.identifier
            );
            println!(
                "doctor_repair_dry_run action=set_state issue={} target_state=need_human_input",
                issue.identifier
            );
        }
    }

    if repair.mark_pr_ready {
        if !repair.confirm_handoff_ready {
            println!(
                "doctor_repair_dry_run action=blocked issue={} reason=missing_confirm_handoff_ready",
                issue.identifier
            );
            if repair.write {
                return Err(
                    "doctor repair --mark-pr-ready requires --confirm-handoff-ready".into(),
                );
            }
            return Ok(());
        }
        let pr_ref = draft_pull_request_repair_target(issue)?;
        let workpad = render_doctor_repair_workpad(issue, report, "mark_pr_ready");
        if repair.write {
            adapter.add_issue_comment(&issue.identifier, &workpad)?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor repair",
                    mutation_type: "timeline_comment",
                    issue_ref: Some(&issue.identifier),
                    target: Some(pr_ref.clone()),
                    from_state: Some(issue.state.clone()),
                    to_state: Some(issue.state.clone()),
                    reason: "doctor repair evidence before PR ready mutation",
                },
            );
            let ready = ensure_pull_request_ready(
                &pr_ref,
                &ProcessHandoffCommandRunner,
                &std::env::current_dir()?,
            )?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor repair",
                    mutation_type: "pr_ready",
                    issue_ref: Some(&issue.identifier),
                    target: Some(ready.pr_url.clone()),
                    from_state: Some(issue.state.clone()),
                    to_state: Some(issue.state.clone()),
                    reason: "operator-confirmed draft PR handoff repair",
                },
            );
            println!(
                "doctor_repair_action=mark_pr_ready issue={} url={} was_draft={} marked_ready={}",
                issue.identifier, ready.pr_url, ready.was_draft, ready.marked_ready
            );
        } else {
            println!(
                "doctor_repair_dry_run action=timeline_comment issue={} evidence=doctor_repair_mark_pr_ready",
                issue.identifier
            );
            println!(
                "doctor_repair_dry_run action=pr_ready issue={} pr_ref={} requires=confirm_handoff_ready",
                issue.identifier, pr_ref
            );
        }
    }

    Ok(())
}

fn draft_pull_request_repair_target(
    issue: &TrackerIssue,
) -> Result<String, Box<dyn std::error::Error>> {
    issue
        .linked_pull_requests
        .iter()
        .find(|pr| pr.is_draft == Some(true))
        .and_then(|pr| {
            pr.url
                .clone()
                .or_else(|| pr.number.map(|number| format!("#{number}")))
        })
        .ok_or_else(|| {
            format!(
                "doctor repair could not find a linked draft PR for {}",
                issue.identifier
            )
            .into()
        })
}

fn issue_ref_matches(left: &str, right: &str) -> bool {
    left.trim().trim_start_matches('#') == right.trim().trim_start_matches('#')
}

fn doctor_repair_human_review(
    workflow_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = adapter.fetch_issues_by_states(&all_mapped_tracker_states(&config))?;
    let issues = hydrate_issues_for_doctor(adapter.as_ref(), issues)?;
    let report = audit_project_issues(&issues);
    let candidates = human_review_repair_candidates(&report);

    println!(
        "doctor_repair_human_review candidates={} write={write}",
        candidates.len()
    );
    for violation in candidates {
        println!(
            "doctor_repair_human_review action=move issue={} from={:?} to=agent_review",
            violation.issue_ref, violation.state
        );
        if write {
            let workpad = render_human_review_repair_workpad(violation);
            adapter.add_issue_comment(&violation.issue_ref, &workpad)?;
            append_tracker_mutation_audit(
                &config,
                TrackerMutationAudit {
                    command: "doctor-repair-human-review",
                    mutation_type: "timeline_comment",
                    issue_ref: Some(&violation.issue_ref),
                    target: None,
                    from_state: Some(violation.state.clone()),
                    to_state: Some("agent_review".into()),
                    reason: "doctor repair evidence",
                },
            );
            adapter.set_state(&violation.issue_ref, "agent_review")?;
            append_tracker_mutation_audit(
                &config,
                TrackerMutationAudit {
                    command: "doctor-repair-human-review",
                    mutation_type: "state_change",
                    issue_ref: Some(&violation.issue_ref),
                    target: None,
                    from_state: Some(violation.state.clone()),
                    to_state: Some("agent_review".into()),
                    reason: "repair invalid Human Review boundary",
                },
            );
        } else {
            println!(
                "doctor_repair_human_review_dry_run action=timeline_comment issue={} evidence=human_review_missing_review_evidence",
                violation.issue_ref
            );
            println!(
                "doctor_repair_human_review_dry_run action=set_state issue={} target_state=agent_review",
                violation.issue_ref
            );
        }
    }

    Ok(())
}

fn all_mapped_tracker_states(config: &RuntimeConfig) -> Vec<String> {
    let state_map = &config.tracker.state_map;
    vec![
        state_map.backlog.clone(),
        state_map.todo.clone(),
        state_map.need_to_clarify.clone(),
        state_map.in_progress.clone(),
        state_map.need_human_input.clone(),
        state_map.agent_review.clone(),
        state_map.human_review.clone(),
        state_map.rework.clone(),
        state_map.merging.clone(),
        state_map.done.clone(),
    ]
}

fn debug_report(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(&workflow_path);
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let integration_gaps = adapter.integration_gaps();
    let project_issues = adapter.list_project_summary_issues()?;
    let doctor_issues = adapter.fetch_issues_by_states(&all_mapped_tracker_states(&config))?;
    let doctor_issues = hydrate_issues_for_doctor(adapter.as_ref(), doctor_issues)?;

    let mut report_gaps = integration_gaps.clone();
    let runtime_states = match load_runtime_states(&config) {
        Ok(states) => states,
        Err(error) => {
            report_gaps.push(format!("runtime_state_load_error: {error}"));
            Vec::new()
        }
    };
    let runtime_state_status = if runtime_states.is_empty() {
        "none".to_string()
    } else if runtime_states.len() == 1 {
        "present".to_string()
    } else {
        format!("present active_workers={}", runtime_states.len())
    };
    let sessions = match session_status_snapshots(&config) {
        Ok(sessions) => sessions,
        Err(error) => {
            report_gaps.push(format!("tmux_session_status_unavailable: {error}"));
            Vec::new()
        }
    };
    let context = ProjectDoctorContext {
        runtime_state: runtime_states.first().cloned(),
        runtime_states,
        sessions: sessions.clone(),
        now_ms: current_time_ms(),
        stale_after_ms: 10_800_000,
    };
    let mut doctor_report = audit_project_issues_with_context(&doctor_issues, Some(&context));
    doctor_report.integration_gaps = report_gaps.clone();
    append_workspace_doctor_violations(&mut doctor_report, &config, &doctor_issues);

    let dogfood_gap_report = classify_dogfood_integration_gaps(&integration_gaps);
    let controlled_candidates = project_issues
        .iter()
        .filter(|issue| is_controlled_dogfood_smoke_issue(issue))
        .count();
    let executable_candidates = project_issues
        .iter()
        .filter(|issue| {
            is_controlled_dogfood_smoke_issue(issue)
                && evaluate_issue_for_current_source(&config, issue)
                    .map(|decision| decision.is_dispatchable())
                    .unwrap_or(false)
        })
        .count();
    let fixture_mode = config.tracker.fixture_path.is_some();
    let supervised_ready =
        !fixture_mode && dogfood_gap_report.blocking.is_empty() && executable_candidates > 0;

    let cleanup = cleanup_plan(&config, &doctor_issues);
    let removable_cleanup = cleanup
        .candidates
        .iter()
        .filter(|candidate| candidate.removable)
        .count();
    let cleanup_needs_decision = cleanup
        .candidates
        .iter()
        .filter(|candidate| !candidate.removable && candidate.path.exists())
        .count();

    println!("Jade Symphony Debug Report");
    println!("read_only=true");
    println!("workflow={}", workflow_path.display());
    println!("tracker_kind={}", config.tracker.kind);
    println!("fixture_mode={fixture_mode}");
    println!();

    println!("Project");
    println!("project_state_access=ok");
    println!("trusted=true");
    println!("issues={}", project_issues.len());
    println!("empty_queue={}", project_issues.is_empty());
    println!("{}", render_state_summary(&project_issues));
    println!("integration_gaps={}", integration_gaps.len());
    for gap in &integration_gaps {
        println!("- integration_gap={gap}");
    }
    println!();

    println!("Doctor");
    println!("doctor_health={}", doctor_health_label(&doctor_report));
    println!("doctor_issues={}", doctor_report.total_issues);
    println!("doctor_violations={}", doctor_report.violations.len());
    println!("doctor_blockers={}", doctor_report.blocker_count());
    for violation in doctor_report.violations.iter().take(5) {
        println!(
            "- {} state={} severity={:?} code={} message={}",
            violation.issue_ref,
            violation.state,
            violation.severity,
            violation.code,
            violation.message
        );
    }
    if doctor_report.violations.len() > 5 {
        println!("- more_violations={}", doctor_report.violations.len() - 5);
    }
    println!();

    println!("Smoke Readiness");
    println!("controlled_candidates={controlled_candidates}");
    println!("executable_candidates={executable_candidates}");
    println!(
        "integration_gap_blocking_count={}",
        dogfood_gap_report.blocking.len()
    );
    println!(
        "integration_gap_warning_count={}",
        dogfood_gap_report.warnings.len()
    );
    let smoke_gate = main_app_server_smoke_gate(&config);
    println!("main_backend={}", smoke_gate.backend);
    println!("main_backend_source={}", smoke_gate.backend_source);
    println!(
        "main_backend_command={}",
        shell_quote_display(&smoke_gate.command)
    );
    println!(
        "main_backend_approval_policy={}",
        smoke_gate.approval_policy
    );
    println!(
        "app_server_live_smoke_ready={}",
        smoke_gate.app_server_live_smoke_ready
    );
    println!(
        "app_server_live_smoke_reason={}",
        smoke_gate.app_server_live_smoke_reason
    );
    println!(
        "app_server_live_smoke_dry_run_command=\"cargo run -- main loop {} --max-iterations 1 --dry-run\"",
        workflow_path.display()
    );
    println!(
        "app_server_live_smoke_write_command=\"cargo run -- main loop {} --max-iterations 1 --write\"",
        workflow_path.display()
    );
    println!("supervised_ready={supervised_ready}");
    println!("unattended_ready=false");
    println!("unattended_reason=Jade Symphony CLI still requires supervised lane commands for dogfood and repair decisions.");
    println!();

    println!("Runtime And Sessions");
    println!(
        "runtime_state_path={}",
        runtime_state_path(&config).display()
    );
    println!("runtime_state={runtime_state_status}");
    println!(
        "session_registry={}",
        session_registry_path(&config).display()
    );
    println!("tmux_sessions={}", sessions.len());
    println!(
        "session_status_summary={}",
        session_status_summary(&sessions)
    );
    println!(
        "event_log={}",
        config
            .observability
            .logs_root
            .join("jade-symphony.jsonl")
            .display()
    );
    println!();

    println!("Cleanup And Audit");
    println!("workspace_root={}", cleanup.workspace_root.display());
    println!("cleanup_candidates={removable_cleanup}");
    println!("needs_human_decision={cleanup_needs_decision}");
    println!("clean_write_supported=false");
    println!();

    println!("Lane Next Actions");
    print_debug_lane_next_actions(&workflow_path, &project_issues, &doctor_report, &sessions);
    println!();

    println!("Tracker Authority");
    println!("authority=Jade Symphony CLI Project reads and mutations are the operator authority for Project state.");
    println!(
        "project_state_command=cargo run -- project state {}",
        workflow_path.display()
    );
    println!(
        "doctor_command=cargo run -- doctor {}",
        workflow_path.display()
    );
    println!(
        "clean_audit_command=cargo run -- clean audit {}",
        workflow_path.display()
    );

    Ok(())
}

fn doctor_health_label(report: &ProjectAuditReport) -> &'static str {
    if report.blocker_count() > 0 {
        "blocked"
    } else if report.violations.is_empty() {
        "clean"
    } else {
        "needs_attention"
    }
}

fn session_status_summary(sessions: &[SessionStatusSnapshot]) -> String {
    let mut counts = BTreeMap::new();
    for session in sessions {
        *counts.entry(session.status.as_str()).or_insert(0usize) += 1;
    }
    if counts.is_empty() {
        return "none".into();
    }
    counts
        .into_iter()
        .map(|(status, count)| format!("{status}:{count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn issue_state_count(issues: &[TrackerIssue], state: &str) -> usize {
    let normalized = normalize_state(state);
    issues
        .iter()
        .filter(|issue| issue.normalized_state() == normalized)
        .count()
}

fn first_issue_in_state<'a>(issues: &'a [TrackerIssue], state: &str) -> Option<&'a TrackerIssue> {
    let normalized = normalize_state(state);
    issues
        .iter()
        .find(|issue| issue.normalized_state() == normalized)
}

fn active_lane_claim_count(
    issues: &[TrackerIssue],
    field_name: &str,
    lane: LaneClaimLane,
    states: &[&str],
) -> usize {
    let normalized_states = states
        .iter()
        .map(|state| normalize_state(state))
        .collect::<Vec<_>>();
    issues
        .iter()
        .filter(|issue| {
            normalized_states
                .iter()
                .any(|state| state == &issue.normalized_state())
        })
        .filter(|issue| {
            project_text_field(issue, field_name)
                .and_then(|value| LaneClaim::parse(&value).ok())
                .map(|claim| claim.lane == lane && claim.state == LaneClaimState::Active)
                .unwrap_or(false)
        })
        .count()
}

fn print_debug_lane_next_actions(
    workflow_path: &Path,
    issues: &[TrackerIssue],
    doctor_report: &ProjectAuditReport,
    sessions: &[SessionStatusSnapshot],
) {
    let todo = issue_state_count(issues, "Todo");
    let rework = issue_state_count(issues, "Rework");
    let in_progress = issue_state_count(issues, "In Progress");
    let agent_review = issue_state_count(issues, "Agent Review");
    let merging = issue_state_count(issues, "Merging");
    let need_to_clarify = issue_state_count(issues, "Need to Clarify");
    let backlog = issue_state_count(issues, "Backlog");
    let active_main_claims = active_lane_claim_count(
        issues,
        "Main Agent",
        LaneClaimLane::Main,
        &["Todo", "Rework", "In Progress"],
    );
    let active_review_claims = active_lane_claim_count(
        issues,
        "Review Agent",
        LaneClaimLane::Review,
        &["Agent Review"],
    );
    let active_merge_claims =
        active_lane_claim_count(issues, "Merging Agent", LaneClaimLane::Merge, &["Merging"]);
    let sessions_need_attention = sessions
        .iter()
        .filter(|session| {
            matches!(
                session.status.as_str(),
                "waiting_for_approval"
                    | "waiting_for_human_input"
                    | "waiting_for_trust"
                    | "usage_limited"
                    | "failed"
                    | "stale"
            )
        })
        .count();

    println!(
        "- Main lane: todo={todo} rework={rework} in_progress={in_progress} active_claims={active_main_claims}"
    );
    if todo + rework > 0 {
        println!(
            "  next=cargo run -- main loop {} --max-iterations 1 --write",
            workflow_path.display()
        );
    } else if in_progress > 0 {
        if let Some(issue) = first_issue_in_state(issues, "In Progress") {
            println!(
                "  next=cargo run -- workspace show {} {}",
                workflow_path.display(),
                issue.identifier
            );
        }
    } else {
        println!("  next=no_main_lane_dispatchable_work");
    }

    println!("- Review lane: agent_review={agent_review} active_claims={active_review_claims}");
    if agent_review > 0 {
        println!(
            "  next=cargo run -- review loop {} --max-iterations 1 --write",
            workflow_path.display()
        );
    } else {
        println!("  next=no_agent_review_items");
    }

    println!("- Merge lane: merging={merging} active_claims={active_merge_claims}");
    if merging > 0 {
        println!(
            "  next=cargo run -- merge loop {} --max-iterations 1 --write",
            workflow_path.display()
        );
    } else {
        println!("  next=no_merging_items");
    }

    println!("- Issue Forge: backlog={backlog} need_to_clarify={need_to_clarify}");
    if need_to_clarify > 0 {
        println!("  next=answer clarification prompts before dispatch");
    } else if backlog > 0 {
        println!(
            "  next=cargo run -- forge promote <issue> --workflow {} --write",
            workflow_path.display()
        );
    } else {
        println!("  next=no_backlog_or_clarification_items");
    }

    println!(
        "- Doctor/Clean: blockers={} warnings={} sessions_needing_attention={sessions_need_attention}",
        doctor_report.blocker_count(),
        doctor_report
            .violations
            .len()
            .saturating_sub(doctor_report.blocker_count())
    );
    if doctor_report.blocker_count() > 0 || sessions_need_attention > 0 {
        println!(
            "  next=cargo run -- doctor {} --interactive",
            workflow_path.display()
        );
    } else {
        println!(
            "  next=cargo run -- clean audit {}",
            workflow_path.display()
        );
    }
}

fn list_profiles(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let profiles = discover_execution_profiles(&config.profiles)?;
    let selected = selected_execution_profile(&config.profiles)?;

    println!("profiles={}", profiles.len());
    if let Some(profile) = selected {
        println!("selected_profile={}", profile.profile_id);
        println!("selected_instance={}", profile.instance_name);
    }
    for profile in profiles {
        println!(
            "- profile_id={} instance_name={} source={} workspace_namespace={} backend={}",
            profile.profile_id,
            profile.instance_name,
            profile.source,
            profile.workspace_namespace,
            profile.backend.as_deref().unwrap_or("configured")
        );
    }
    Ok(())
}

fn cleanup_plan_command(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let states = config.tracker.terminal_states.clone();
    let issues = adapter.fetch_issues_by_states(&states)?;
    let layout = artifact_layout(&config);
    let plan = cleanup_plan(&config, &issues);

    println!("cleanup_plan=dry_run");
    println!("artifact_root={}", layout.root.display());
    println!("artifact_namespace={}", layout.namespace);
    println!("artifact_profile={}", layout.profile_namespace);
    println!(
        "artifact_class=per_issue_worktree path={}",
        layout.class_path(ArtifactClass::PerIssueWorktree).display()
    );
    println!(
        "artifact_class=runtime_state path={}",
        layout.class_path(ArtifactClass::RuntimeState).display()
    );
    println!(
        "artifact_class=event_log path={}",
        layout.class_path(ArtifactClass::EventLog).display()
    );
    println!(
        "artifact_class=rendered_agent_prompt path={}",
        layout
            .class_path(ArtifactClass::RenderedAgentPrompt)
            .join("prompts")
            .display()
    );
    println!(
        "artifact_class=review_job_artifact path={}",
        layout
            .class_path(ArtifactClass::ReviewJobArtifact)
            .display()
    );
    println!(
        "artifact_class=pr_body_draft path={}",
        layout
            .class_path(ArtifactClass::PullRequestBodyDraft)
            .display()
    );
    println!(
        "artifact_class=workpad_draft path={}",
        layout.class_path(ArtifactClass::WorkpadDraft).display()
    );
    println!(
        "artifact_class=reusable_workflow_prompt path={}",
        layout
            .class_path(ArtifactClass::ReusableWorkflowPrompt)
            .display()
    );
    println!(
        "artifact_class=disposable_scratch path={}",
        layout
            .class_path(ArtifactClass::DisposableScratch)
            .display()
    );
    println!("{}", render_cleanup_plan(&plan));
    println!("cleanup_plan_write_supported=false");
    Ok(())
}

fn render_cleanup_plan(plan: &CleanupPlan) -> String {
    let mut lines = vec![
        format!("workspace_root={}", plan.workspace_root.display()),
        format!("cleanup_candidates={}", plan.candidates.len()),
    ];

    for candidate in &plan.candidates {
        lines.push(format!(
            "- issue={} state={} removable={} path={}",
            candidate.issue_identifier,
            candidate.issue_state,
            candidate.removable,
            candidate.path.display()
        ));
        lines.push(format!(
            "  branch={}",
            candidate.branch.as_deref().unwrap_or("unknown")
        ));
        lines.push(format!(
            "  linked_pr_state={}",
            candidate.linked_pr_state.as_deref().unwrap_or("none")
        ));
        for reason in &candidate.reasons {
            lines.push(format!("  reason={reason}"));
        }
        for blocker in &candidate.blockers {
            lines.push(format!("  blocker={blocker}"));
        }
    }

    lines.join("\n")
}

fn clean_audit_command(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let terminal_issues = adapter.fetch_issues_by_states(&config.tracker.terminal_states)?;
    let layout = artifact_layout(&config);
    let plan = cleanup_plan(&config, &terminal_issues);
    let sessions = session_status_snapshots(&config).unwrap_or_else(|error| {
        println!("clean_audit_warning kind=tmux_session_status reason={error}");
        Vec::new()
    });

    println!("clean_audit=read_only");
    println!("artifact_root={}", layout.root.display());
    println!("workspace_root={}", config.workspace.root.display());
    print_clean_audit_path(
        "safe_to_keep",
        "session_registry",
        session_registry_path(&config),
        "durable tmux session evidence until session state is reconciled",
    );
    print_clean_audit_path(
        "safe_to_keep",
        "runtime_state",
        layout.class_path(ArtifactClass::RuntimeState),
        "resume-critical while an issue is active",
    );
    print_clean_audit_path(
        "safe_to_keep",
        "event_log",
        layout.class_path(ArtifactClass::EventLog),
        "local execution evidence",
    );
    print_clean_audit_path(
        "attach_to_tracker",
        "rendered_agent_prompt",
        config.observability.logs_root.join("prompts"),
        "prompt artifacts should stay available until tracker evidence names the run",
    );
    print_clean_audit_path(
        "safe_to_keep",
        "tmux_log",
        config.observability.logs_root.join("tmux"),
        "tmux logs are operator recovery evidence for interrupted sessions",
    );
    print_clean_audit_path(
        "safe_to_keep",
        "review_job_artifact",
        layout.class_path(ArtifactClass::ReviewJobArtifact),
        "review evidence until tracker workpad records it",
    );
    print_clean_audit_path(
        "attach_to_tracker",
        "pr_body_draft",
        layout.class_path(ArtifactClass::PullRequestBodyDraft),
        "draft should be represented by a pull request or issue workpad",
    );
    print_clean_audit_path(
        "attach_to_tracker",
        "workpad_draft",
        layout.class_path(ArtifactClass::WorkpadDraft),
        "draft should be represented by tracker-visible evidence",
    );
    print_clean_audit_path(
        "promote_to_repo",
        "reusable_workflow_prompt",
        layout.class_path(ArtifactClass::ReusableWorkflowPrompt),
        "workflow and prompt material should live in repo docs, examples, or workflows",
    );
    print_clean_audit_path(
        "cleanup_candidate",
        "disposable_scratch",
        layout.class_path(ArtifactClass::DisposableScratch),
        "scratch files are disposable after operator review",
    );
    print_clean_audit_path(
        "needs_human_decision",
        "canonical_checkout_quarantine",
        canonical_quarantine_root(&config),
        "files moved out of the canonical checkout before live write lanes should be archived or deleted after tracker evidence is settled",
    );

    let mut cleanup_candidates = 0;
    let mut human_decisions = 0;
    for candidate in &plan.candidates {
        if !candidate.path.exists() {
            continue;
        }
        if candidate.removable {
            cleanup_candidates += 1;
            println!(
                "clean_audit_item category=cleanup_candidate kind=worktree issue={} path={} reason=terminal_issue_clean_merged_or_closed",
                candidate.issue_identifier,
                candidate.path.display()
            );
        } else {
            human_decisions += 1;
            println!(
                "clean_audit_item category=needs_human_decision kind=worktree issue={} path={} reason={}",
                candidate.issue_identifier,
                candidate.path.display(),
                clean_audit_blocker_summary(candidate)
            );
        }
    }
    for session in &sessions {
        if session.status == "completed" {
            cleanup_candidates += 1;
            println!(
                "clean_audit_item category=cleanup_candidate kind=tmux_session issue={} session={} log={} prompt=unknown reason=session_completed_and_registry_evidence_present",
                session.issue_identifier.as_deref().unwrap_or("n/a"),
                session.session_id,
                session.log_path.as_deref().unwrap_or("n/a")
            );
        } else {
            human_decisions += 1;
            println!(
                "clean_audit_item category=needs_human_decision kind=tmux_session issue={} session={} status={} attach={} log={} reason=session_not_completed",
                session.issue_identifier.as_deref().unwrap_or("n/a"),
                session.session_id,
                session.status,
                session.attach_command.as_deref().unwrap_or("n/a"),
                session.log_path.as_deref().unwrap_or("n/a")
            );
        }
    }
    println!(
        "clean_audit_summary cleanup_candidates={cleanup_candidates} needs_human_decision={human_decisions}"
    );
    println!("clean_audit_write_supported=false");
    Ok(())
}

fn print_clean_audit_path(category: &str, kind: &str, path: impl AsRef<Path>, reason: &str) {
    let path = path.as_ref();
    let entries = read_dir_entry_count(path);
    println!(
        "clean_audit_item category={category} kind={kind} path={} exists={} entries={entries} reason={reason}",
        path.display(),
        path.exists()
    );
}

fn read_dir_entry_count(path: &Path) -> usize {
    fs::read_dir(path)
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0)
}

fn clean_audit_blocker_summary(candidate: &jade_symphony::artifacts::CleanupCandidate) -> String {
    if !candidate.blockers.is_empty() {
        return candidate.blockers.join(",");
    }
    if !candidate.reasons.is_empty() {
        return candidate.reasons.join(",");
    }
    "operator_review_required".into()
}

fn cleanup_workspaces(
    workflow_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issues = adapter.fetch_issues_by_states(&config.tracker.terminal_states)?;
    let entries = workspace_cleanup_plan(&config, &issues)?;
    let eligible = entries
        .iter()
        .filter(|entry| matches!(entry.action, WorkspaceCleanupAction::Eligible))
        .count();

    println!(
        "workspace_cleanup mode={} terminal_issues={} eligible={eligible}",
        if write { "write" } else { "dry-run" },
        issues.len()
    );

    for entry in &entries {
        println!(
            "workspace_cleanup issue={} state={:?} action={} workspace_key={} path={}",
            entry.issue_ref,
            entry.state,
            entry.action.label(),
            entry.workspace_key,
            entry.workspace_path.display()
        );
        if let WorkspaceCleanupAction::Skipped { reason } = &entry.action {
            println!(
                "workspace_cleanup_skip issue={} reason={}",
                entry.issue_ref, reason
            );
        }
    }

    if write {
        for entry in entries
            .iter()
            .filter(|entry| matches!(entry.action, WorkspaceCleanupAction::Eligible))
        {
            remove_issue_workspace(&config.workspace.root, &entry.workspace_key, &config.hooks)?;
            println!(
                "workspace_cleanup_removed issue={} path={}",
                entry.issue_ref,
                entry.workspace_path.display()
            );
        }
    }

    Ok(())
}

fn workspace_list(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issues = adapter.fetch_issues_by_states(&all_mapped_tracker_states(&config))?;
    let registry = load_session_registry(&session_registry_path(&config))?;
    let worktrees = git_worktree_list(&std::env::current_dir()?)?;
    let mut shown = 0usize;

    for issue in &issues {
        let report = discover_issue_workspaces_from_parts(
            issue,
            &registry.sessions,
            &worktrees,
            &config.tracker.workpad.marker,
        );
        if report.candidates.is_empty() {
            continue;
        }
        shown += 1;
        println!(
            "workspace_list issue={} state={:?} candidates={} canonical={}",
            issue.identifier,
            issue.state,
            report.candidates.len(),
            report
                .canonical_index
                .and_then(|index| report.candidates.get(index))
                .map(|candidate| candidate.path.display().to_string())
                .unwrap_or_else(|| "none".into())
        );
        for candidate in &report.candidates {
            println!(
                "workspace_candidate issue={} strength={} branch={} path={} evidence={}",
                issue.identifier,
                candidate.strength.as_str(),
                candidate.branch.as_deref().unwrap_or("unknown"),
                candidate.path.display(),
                evidence_summary(candidate)
            );
        }
    }

    for worktree in worktrees {
        if let Some(issue_ref) =
            infer_issue_ref_from_branch_or_path(worktree.branch.as_deref(), &worktree.path)
        {
            if !issues.iter().any(|issue| issue.identifier == issue_ref) {
                println!(
                    "workspace_orphan_hint issue={} branch={} path={}",
                    issue_ref,
                    worktree.branch.as_deref().unwrap_or("unknown"),
                    worktree.path.display()
                );
            }
        }
    }

    if shown == 0 {
        println!("workspace_list=empty");
    }
    Ok(())
}

fn workspace_show(
    workflow_path: PathBuf,
    issue_ref: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let registry = load_session_registry(&session_registry_path(&config))?;
    let worktrees = git_worktree_list(&std::env::current_dir()?)?;
    let report = discover_issue_workspaces_from_parts(
        &issue,
        &registry.sessions,
        &worktrees,
        &config.tracker.workpad.marker,
    );
    print_workspace_report(&report);
    Ok(())
}

fn workspace_adopt(
    workflow_path: PathBuf,
    issue_ref: String,
    path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let worktrees = git_worktree_list(&std::env::current_dir()?)?;
    let candidate = validate_workspace_adoption(&issue, &path, &worktrees)?;
    let workpad =
        render_workspace_adoption_workpad(&issue, &config.tracker.workpad.marker, &candidate);

    if !write {
        println!(
            "workspace_adopt_dry_run issue={} branch={} path={}",
            issue.identifier,
            candidate.branch.as_deref().unwrap_or("unknown"),
            candidate.path.display()
        );
        return Ok(());
    }

    adapter.upsert_workpad(&issue.identifier, &workpad)?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "workspace-adopt",
            mutation_type: "workpad",
            issue_ref: Some(&issue.identifier),
            target: Some(format!("workspace={}", candidate.path.display())),
            from_state: Some(issue.state.clone()),
            to_state: None,
            reason: "operator selected canonical issue worktree",
        },
    );
    println!(
        "workspace_adopt=ok issue={} branch={} path={}",
        issue.identifier,
        candidate.branch.as_deref().unwrap_or("unknown"),
        candidate.path.display()
    );
    Ok(())
}

fn workspace_ensure(
    workflow_path: PathBuf,
    issue_ref: String,
    pr_ref: Option<String>,
    branch: Option<String>,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(&config, "workspace ensure", write)?;

    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let repo_root = std::env::current_dir()?;
    let registry = load_session_registry(&session_registry_path(&config))?;
    let worktrees = git_worktree_list(&repo_root)?;
    let report = discover_issue_workspaces_from_parts(
        &issue,
        &registry.sessions,
        &worktrees,
        &config.tracker.workpad.marker,
    );

    if report
        .warnings
        .iter()
        .any(|warning| warning.contains("multiple strong"))
    {
        return Err(format!(
            "workspace ensure refuses ambiguous candidates for {}; run `workspace show` and resolve with `workspace adopt`",
            issue.identifier
        )
        .into());
    }

    if let Some(candidate) = report
        .canonical_index
        .and_then(|index| report.candidates.get(index))
        .cloned()
    {
        ensure_existing_candidate_clean(&candidate)?;
        let branch = candidate.branch.as_deref().unwrap_or("unknown");
        if !write {
            println!(
                "workspace_ensure_dry_run action=reuse issue={} branch={} path={}",
                issue.identifier,
                branch,
                candidate.path.display()
            );
            return Ok(());
        }
        let pr_label = workspace_ensure_pr_label(&issue, pr_ref.as_deref());
        let workpad = render_workspace_ensure_workpad(
            &issue,
            &config.tracker.workpad.marker,
            &candidate,
            "reused",
            pr_label.as_deref(),
        );
        adapter.upsert_workpad(&issue.identifier, &workpad)?;
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "workspace ensure",
                mutation_type: "workpad",
                issue_ref: Some(&issue.identifier),
                target: Some(format!("workspace={}", candidate.path.display())),
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "reused safe Review/Merge inspection worktree",
            },
        );
        println!(
            "workspace_ensure=ok action=reused issue={} branch={} path={}",
            issue.identifier,
            branch,
            candidate.path.display()
        );
        return Ok(());
    }

    let plan = run_loop_handoff_plan(&config, &issue)?;
    let workspace_path = plan.workspace_path.clone();
    validate_workspace_path_under_root(&config.workspace.root, &workspace_path)?;
    let branch_name = workspace_ensure_branch(&issue, branch, &plan.branch_name)?;
    let pr_number = workspace_ensure_pr_number(&issue, pr_ref.as_deref());
    let pr_label = pr_ref
        .clone()
        .or_else(|| pr_number.map(|number| format!("#{number}")));

    if !write {
        println!(
            "workspace_ensure_dry_run action=create issue={} branch={} path={} workspace_root={}",
            issue.identifier,
            branch_name,
            workspace_path.display(),
            config.workspace.root.display()
        );
        return Ok(());
    }

    ensure_inspection_worktree(&repo_root, &workspace_path, &branch_name, pr_number)?;
    let worktrees = git_worktree_list(&repo_root)?;
    let candidate =
        validate_workspace_adoption(&issue, &workspace_path, &worktrees).map_err(|error| {
            format!(
                "workspace ensure created or reused {}, but validation failed: {error}",
                workspace_path.display()
            )
        })?;
    ensure_existing_candidate_clean(&candidate)?;
    let workpad = render_workspace_ensure_workpad(
        &issue,
        &config.tracker.workpad.marker,
        &candidate,
        "created",
        pr_label.as_deref(),
    );
    adapter.upsert_workpad(&issue.identifier, &workpad)?;
    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "workspace ensure",
            mutation_type: "workpad",
            issue_ref: Some(&issue.identifier),
            target: Some(format!("workspace={}", candidate.path.display())),
            from_state: Some(issue.state.clone()),
            to_state: None,
            reason: "created safe Review/Merge inspection worktree",
        },
    );
    println!(
        "workspace_ensure=ok action=created issue={} branch={} path={}",
        issue.identifier,
        candidate.branch.as_deref().unwrap_or("unknown"),
        candidate.path.display()
    );
    Ok(())
}

fn ensure_existing_candidate_clean(
    candidate: &IssueWorkspaceCandidate,
) -> Result<(), Box<dyn std::error::Error>> {
    if candidate.branch.is_none() {
        return Err(format!(
            "workspace ensure refuses detached candidate {}; resolve with `workspace adopt` after choosing a branch worktree",
            candidate.path.display()
        )
        .into());
    }
    let status = ProcessCommand::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&candidate.path)
        .output()?;
    if !status.status.success() {
        return Err(format!(
            "workspace ensure could not inspect candidate {}: {}",
            candidate.path.display(),
            String::from_utf8_lossy(&status.stderr).trim()
        )
        .into());
    }
    let dirty = String::from_utf8_lossy(&status.stdout).trim().to_string();
    if !dirty.is_empty() {
        return Err(format!(
            "workspace ensure refuses dirty candidate {}: {}",
            candidate.path.display(),
            dirty.replace('\n', "; ")
        )
        .into());
    }
    Ok(())
}

fn workspace_ensure_branch(
    issue: &TrackerIssue,
    explicit_branch: Option<String>,
    fallback_branch: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(branch) = explicit_branch.filter(|branch| !branch.trim().is_empty()) {
        return Ok(branch);
    }
    let linked_heads = issue
        .linked_pull_requests
        .iter()
        .filter_map(|pr| pr.head_ref_name.as_deref())
        .filter(|head| !head.trim().is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    if linked_heads.len() > 1 {
        return Err(format!(
            "workspace ensure found multiple linked PR head branches for {}; pass --branch or resolve the linked PR",
            issue.identifier
        )
        .into());
    }
    if let Some(head) = linked_heads.iter().next() {
        return Ok((*head).to_string());
    }
    if let Some(branch) = issue
        .branch_name
        .as_deref()
        .filter(|branch| !branch.is_empty())
    {
        return Ok(branch.to_string());
    }
    Ok(fallback_branch.to_string())
}

fn workspace_ensure_pr_number(issue: &TrackerIssue, explicit_pr: Option<&str>) -> Option<u64> {
    explicit_pr
        .and_then(pull_request_number_from_url)
        .or_else(|| {
            let mut numbers = issue
                .linked_pull_requests
                .iter()
                .filter_map(|pr| pr.number)
                .collect::<std::collections::BTreeSet<_>>();
            if numbers.len() == 1 {
                numbers.pop_first()
            } else {
                None
            }
        })
}

fn workspace_ensure_pr_label(issue: &TrackerIssue, explicit_pr: Option<&str>) -> Option<String> {
    explicit_pr
        .map(str::to_string)
        .or_else(|| workspace_ensure_pr_number(issue, None).map(|number| format!("#{number}")))
}

fn validate_workspace_path_under_root(
    root: &Path,
    workspace_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let parent = workspace_path.parent().unwrap_or(root.as_path());
    let canonical_parent = parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf());
    if workspace_path == root || !canonical_parent.starts_with(&root) {
        return Err(format!(
            "workspace ensure path {} escapes workflow workspace root {}",
            workspace_path.display(),
            root.display()
        )
        .into());
    }
    Ok(())
}

fn ensure_inspection_worktree(
    repo_root: &Path,
    workspace_path: &Path,
    branch_name: &str,
    pr_number: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    if workspace_path.exists() {
        let candidate = IssueWorkspaceCandidate {
            path: workspace_path.to_path_buf(),
            branch: current_git_branch(workspace_path)?,
            head: None,
            strength: WorkspaceMatchStrength::Strong,
            evidence: Vec::new(),
        };
        ensure_existing_candidate_clean(&candidate)?;
        return Ok(());
    }
    if let Some(parent) = workspace_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let branch_ref = format!("refs/heads/{branch_name}");
    let branch_exists = ProcessCommand::new("git")
        .args(["show-ref", "--verify", "--quiet", &branch_ref])
        .current_dir(repo_root)
        .status()?
        .success();
    if !branch_exists {
        if let Some(number) = pr_number {
            let fetch_ref = format!("pull/{number}/head:{branch_name}");
            let output = ProcessCommand::new("git")
                .args(["fetch", "origin", &fetch_ref])
                .current_dir(repo_root)
                .output()?;
            if !output.status.success() {
                return Err(format!(
                    "workspace ensure failed to fetch PR #{number}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )
                .into());
            }
        }
    }
    let mut args = vec!["worktree", "add"];
    let workspace_arg = workspace_path.display().to_string();
    if branch_exists || pr_number.is_some() {
        args.push(&workspace_arg);
        args.push(branch_name);
        let output = ProcessCommand::new("git")
            .args(&args)
            .current_dir(repo_root)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "workspace ensure failed to add worktree: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        return Ok(());
    }

    let output = ProcessCommand::new("git")
        .args(["worktree", "add", "-b", branch_name, &workspace_arg, "main"])
        .current_dir(repo_root)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "workspace ensure failed to create worktree branch `{branch_name}`: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(())
}

fn print_workspace_report(report: &IssueWorkspaceReport) {
    println!(
        "workspace_show issue={} candidates={} canonical={}",
        report.issue_ref,
        report.candidates.len(),
        report
            .canonical_index
            .and_then(|index| report.candidates.get(index))
            .map(|candidate| candidate.path.display().to_string())
            .unwrap_or_else(|| "none".into())
    );
    if !report.branch_hints.is_empty() {
        println!("workspace_branch_hints {}", report.branch_hints.join(","));
    }
    for warning in &report.warnings {
        println!(
            "workspace_warning issue={} message={}",
            report.issue_ref, warning
        );
    }
    for candidate in &report.candidates {
        println!(
            "workspace_candidate issue={} strength={} branch={} head={} path={} evidence={}",
            report.issue_ref,
            candidate.strength.as_str(),
            candidate.branch.as_deref().unwrap_or("unknown"),
            candidate.head.as_deref().unwrap_or("unknown"),
            candidate.path.display(),
            evidence_summary(candidate)
        );
    }
}

fn evidence_summary(candidate: &IssueWorkspaceCandidate) -> String {
    candidate
        .evidence
        .iter()
        .map(|evidence| format!("{}:{}", evidence.source, evidence.detail.replace(' ', "_")))
        .collect::<Vec<_>>()
        .join("|")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceCleanupEntry {
    issue_ref: String,
    state: String,
    workspace_key: String,
    workspace_path: PathBuf,
    action: WorkspaceCleanupAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkspaceCleanupAction {
    Eligible,
    Skipped { reason: String },
}

impl WorkspaceCleanupAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::Skipped { .. } => "skipped",
        }
    }
}

fn workspace_cleanup_plan(
    config: &RuntimeConfig,
    issues: &[TrackerIssue],
) -> Result<Vec<WorkspaceCleanupEntry>, Box<dyn std::error::Error>> {
    let terminal_states = config.terminal_state_set();
    let profile = selected_execution_profile(&config.profiles)?;
    let profile_namespace = profile
        .as_ref()
        .map(|profile| profile.workspace_namespace.as_str());

    let mut entries = Vec::new();
    for issue in issues {
        if !terminal_states.contains(&issue.normalized_state()) {
            entries.push(WorkspaceCleanupEntry {
                issue_ref: issue.identifier.clone(),
                state: issue.state.clone(),
                workspace_key: "n/a".into(),
                workspace_path: config.workspace.root.clone(),
                action: WorkspaceCleanupAction::Skipped {
                    reason: "non_terminal_state".into(),
                },
            });
            continue;
        }

        let plan = match plan_issue_handoff_for_profile(
            &config.workspace.root,
            issue,
            DEFAULT_RUN_LOOP_BASE_BRANCH,
            profile_namespace,
        ) {
            Ok(plan) => plan,
            Err(error) => {
                entries.push(WorkspaceCleanupEntry {
                    issue_ref: issue.identifier.clone(),
                    state: issue.state.clone(),
                    workspace_key: "n/a".into(),
                    workspace_path: config.workspace.root.clone(),
                    action: WorkspaceCleanupAction::Skipped {
                        reason: format!("handoff_plan_failed:{error}"),
                    },
                });
                continue;
            }
        };

        let action = if plan.workspace_path.exists() {
            WorkspaceCleanupAction::Eligible
        } else {
            WorkspaceCleanupAction::Skipped {
                reason: "workspace_missing".into(),
            }
        };

        entries.push(WorkspaceCleanupEntry {
            issue_ref: issue.identifier.clone(),
            state: issue.state.clone(),
            workspace_key: plan.workspace_key,
            workspace_path: plan.workspace_path,
            action,
        });
    }

    Ok(entries)
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DogfoodIntegrationGapReport {
    blocking: Vec<String>,
    warnings: Vec<String>,
}

fn classify_dogfood_integration_gaps(gaps: &[String]) -> DogfoodIntegrationGapReport {
    let mut report = DogfoodIntegrationGapReport::default();

    for gap in gaps {
        match dogfood_integration_gap_severity(gap) {
            DogfoodIntegrationGapSeverity::Blocking => report.blocking.push(gap.clone()),
            DogfoodIntegrationGapSeverity::Warning => report.warnings.push(gap.clone()),
        }
    }

    report
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DogfoodIntegrationGapSeverity {
    Blocking,
    Warning,
}

fn dogfood_integration_gap_severity(gap: &str) -> DogfoodIntegrationGapSeverity {
    let normalized = gap.to_ascii_lowercase();

    if normalized.contains("pr linking uses an issue comment/autolink strategy")
        || normalized.contains("pull request linking currently records a tracker comment")
        || normalized.contains("live write methods use `gh api graphql`")
    {
        DogfoodIntegrationGapSeverity::Warning
    } else {
        DogfoodIntegrationGapSeverity::Blocking
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MainAppServerSmokeGate {
    backend: String,
    backend_source: String,
    command: String,
    approval_policy: String,
    app_server_live_smoke_ready: bool,
    app_server_live_smoke_reason: String,
}

fn main_app_server_smoke_gate(config: &RuntimeConfig) -> MainAppServerSmokeGate {
    match config.backend.kind.as_str() {
        "codex" if config.codex.command.contains("app-server") => MainAppServerSmokeGate {
            backend: config.backend.kind.clone(),
            backend_source: "codex-app-server".into(),
            command: config.codex.command.clone(),
            approval_policy: codex_approval_policy_label(config),
            app_server_live_smoke_ready: true,
            app_server_live_smoke_reason:
                "main_lane.backend=codex and codex.command includes app-server".into(),
        },
        "codex" => MainAppServerSmokeGate {
            backend: config.backend.kind.clone(),
            backend_source: "codex-subprocess".into(),
            command: config.codex.command.clone(),
            approval_policy: codex_approval_policy_label(config),
            app_server_live_smoke_ready: false,
            app_server_live_smoke_reason: "codex command does not select the app-server transport"
                .into(),
        },
        "tmux" => MainAppServerSmokeGate {
            backend: config.backend.kind.clone(),
            backend_source: "tmux-fallback".into(),
            command: config
                .tmux
                .main_agent_command
                .clone()
                .unwrap_or_else(|| config.tmux.agent_command.clone()),
            approval_policy: "n/a".into(),
            app_server_live_smoke_ready: false,
            app_server_live_smoke_reason:
                "tmux is explicit fallback/debug and is not the app-server smoke path".into(),
        },
        "dry-run" => MainAppServerSmokeGate {
            backend: config.backend.kind.clone(),
            backend_source: "dry-run".into(),
            command: "dry-run".into(),
            approval_policy: "n/a".into(),
            app_server_live_smoke_ready: false,
            app_server_live_smoke_reason: "dry-run backend cannot perform a live app-server smoke"
                .into(),
        },
        other => MainAppServerSmokeGate {
            backend: config.backend.kind.clone(),
            backend_source: other.into(),
            command: other.into(),
            approval_policy: "n/a".into(),
            app_server_live_smoke_ready: false,
            app_server_live_smoke_reason:
                "configured Main backend is not the Codex app-server runtime".into(),
        },
    }
}

fn codex_approval_policy_label(config: &RuntimeConfig) -> String {
    config
        .codex
        .approval_policy
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| config.codex.approval_policy.to_string())
}

fn is_controlled_dogfood_smoke_issue(issue: &TrackerIssue) -> bool {
    issue
        .labels_lowercase()
        .iter()
        .any(|label| label == "dogfood-smoke" || label == "smoke")
        || issue.title.to_ascii_lowercase().contains("[dogfood-smoke]")
}

fn run_once(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = adapter.list_queue_scan_issues()?;
    let orchestrator = Orchestrator::new(config.clone());
    let plan = orchestrator.plan_dispatch(issues.clone());
    let Some(issue) = plan.selected.first() else {
        println!("{}", render_snapshot(&plan.snapshot));
        println!("run_once=skipped reason=no_dispatchable_issue");
        return Ok(());
    };
    let issue = hydrate_issue_for_evidence(adapter.as_ref(), issue.clone(), &issues)?;

    let result = execute_issue_once(&workflow, &config, &issue)?;

    println!("run_once=completed");
    println!("issue={} {}", issue.identifier, issue.title);
    println!("workspace={}", result.workspace_path.display());
    println!("backend={}", result.backend);
    println!("actor_role={}", result.actor_role);
    println!("actor_label={}", result.actor_label);
    println!(
        "git_author={}",
        result.git_author.as_deref().unwrap_or("n/a")
    );
    println!("git_identity={}", result.git_identity.summary());
    println!("success={}", result.success);
    println!(
        "event_log={}",
        config
            .observability
            .logs_root
            .join("jade-symphony.jsonl")
            .display()
    );
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IssueExecutionResult {
    workspace_path: PathBuf,
    backend: String,
    profile_id: Option<String>,
    instance_name: Option<String>,
    success: bool,
    pending_session: bool,
    session_id: Option<String>,
    run_id: Option<String>,
    backend_log_path: Option<PathBuf>,
    backend_attach_command: Option<String>,
    message: String,
    usage_limit_pause: Option<UsageLimitPause>,
    prompt_artifact_path: Option<PathBuf>,
    actor_role: String,
    actor_label: String,
    git_author: Option<String>,
    git_identity: GitIdentityApplyResult,
    live_handoff: Option<RunLoopLiveHandoff>,
    handoff_verification: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunLoopLiveHandoff {
    worktree: LiveWorktreeResult,
    publication: PullRequestPublication,
    verification: String,
    project_pr_link_verified: Option<bool>,
    pull_request_ready: Option<PullRequestReadyStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HandoffVerification {
    success: bool,
    summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MainSessionReconciliation {
    Terminal(Box<IssueExecutionResult>),
    Active {
        status: String,
        source: String,
        evidence: String,
    },
}

fn main_session_active_recoverable(status: &str, evidence: &str) -> bool {
    status == "stale"
        || (status == "unknown"
            && (evidence.contains("missing from session registry")
                || evidence.contains("without backend session id")
                || evidence.contains("tmux")
                || evidence.contains("unavailable")))
}

fn execute_issue_once(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<IssueExecutionResult, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    let workspace_identifier = profile_scoped_identifier(
        profile
            .as_ref()
            .map(|profile| profile.workspace_namespace.as_str()),
        &issue.identifier,
    );
    execute_issue_once_with_workspace_key(workflow, config, issue, &workspace_identifier, 1, None)
}

fn execute_issue_once_with_workspace_key(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    workspace_key: &str,
    attempt: u32,
    claim: Option<&LaneClaim>,
) -> Result<IssueExecutionResult, Box<dyn std::error::Error>> {
    let workspace = prepare_workspace(&config.workspace.root, workspace_key, &config.hooks)?;
    execute_issue_once_in_workspace(workflow, config, issue, workspace, attempt, claim)
}

fn execute_issue_once_in_workspace(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    workspace: Workspace,
    attempt: u32,
    claim: Option<&LaneClaim>,
) -> Result<IssueExecutionResult, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    let git_identity = apply_local_git_identity(&workspace.path, &config.identity.git)?;
    run_before_run(&workspace.path, &config.hooks)?;

    let mut prompt = render_prompt_with_claim(
        workflow.prompt_for_lane(AgentLane::MainAgent),
        issue,
        None,
        claim,
    )?;
    if config.backend.kind == "codex" && config.codex.command.contains("app-server") {
        prompt.push_str(CODEX_APP_SERVER_HANDOFF_BOUNDARY);
    }
    let backend = backend_from_config(config);
    let mut prepared = backend.prepare(workspace.path.clone(), prompt, config)?;
    prepared.prompt_artifact_path = Some(rendered_prompt_artifact_path(
        config,
        issue,
        prepared.backend.as_str(),
        attempt,
    ));
    prepared.issue_id = Some(issue.id.clone());
    prepared.issue_identifier = Some(issue.identifier.clone());
    prepared.issue_title = Some(issue.title.clone());
    prepared.lane = Some("main".into());
    if let Some(claim) = claim {
        prepared.run_id = Some(claim.run.clone());
        prepared
            .env
            .insert("JADE_SYMPHONY_RUN_ID".into(), claim.run.clone());
        prepared
            .env
            .insert("JADE_SYMPHONY_CLAIM".into(), claim.render());
    }
    prepared.attempt = attempt;
    prepared.branch_name = current_git_branch(&workspace.path).ok().flatten();
    let prompt_artifact_path = persist_prompt_artifact(&prepared)?;
    let mut backend_wait = progress_spec_with_event_log(config, "main_backend")
        .issue(issue.identifier.clone())
        .backend(prepared.backend.clone())
        .next("waiting_for_child");
    if let Some(path) = &prepared.prompt_artifact_path {
        backend_wait = backend_wait.artifact(path.display().to_string());
    }
    let events = run_with_progress_heartbeat(backend_wait, || backend.run(prepared))?;
    let summary = backend.summarize(&events);
    let usage_limit_pause = usage_limit_pause_from_events(&events);
    run_after_run(&workspace.path, &config.hooks);

    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    log.append(&EventRecord {
        event: "prompt_artifact".into(),
        issue_id: Some(issue.id.clone()),
        issue_identifier: Some(issue.identifier.clone()),
        session_id: summary.session_id.clone(),
        profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
        instance_name: profile
            .as_ref()
            .map(|profile| profile.instance_name.clone()),
        actor_role: Some(config.identity.actor_role.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        git_author: config.identity.git.author(),
        tracker_mutation: None,
        message: format!("prompt_artifact={}", prompt_artifact_path.display()),
    })?;
    for event in &events {
        log.append(&EventRecord {
            event: format!("{event:?}"),
            issue_id: Some(issue.id.clone()),
            issue_identifier: Some(issue.identifier.clone()),
            session_id: summary.session_id.clone(),
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
            tracker_mutation: None,
            message: summary.message.clone(),
        })?;
    }

    Ok(IssueExecutionResult {
        workspace_path: workspace.path,
        backend: summary.backend,
        profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
        instance_name: profile
            .as_ref()
            .map(|profile| profile.instance_name.clone()),
        success: summary.success,
        pending_session: summary.pending_session,
        session_id: summary.session_id,
        run_id: claim.map(|claim| claim.run.clone()),
        backend_log_path: summary.log_path,
        backend_attach_command: summary.attach_command,
        message: summary.message,
        usage_limit_pause,
        prompt_artifact_path: Some(prompt_artifact_path),
        actor_role: config.identity.actor_role.clone(),
        actor_label: config.identity.actor_label.clone(),
        git_author: config.identity.git.author(),
        git_identity,
        live_handoff: None,
        handoff_verification: None,
    })
}

fn rendered_prompt_artifact_path(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    backend: &str,
    attempt: u32,
) -> PathBuf {
    config.observability.logs_root.join("prompts").join(format!(
        "{}-attempt-{}-{}-{}.prompt.md",
        safe_identifier(&issue.identifier),
        attempt,
        safe_identifier(backend),
        current_time_ms()
    ))
}

fn current_git_branch(workspace_path: &Path) -> Result<Option<String>, io::Error> {
    let output = ProcessCommand::new("git")
        .args(["branch", "--show-current"])
        .current_dir(workspace_path)
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!branch.is_empty()).then_some(branch))
}

fn latest_status_for_issue(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: &str,
    category: &str,
    action: &str,
    next: Option<String>,
) -> LatestStatus {
    LatestStatus {
        lane: lane.into(),
        category: category.into(),
        action: action.into(),
        issue_identifier: Some(issue.identifier.clone()),
        issue_title: Some(issue.title.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        workspace: None,
        branch: issue.branch_name.clone(),
        session_id: None,
        next,
    }
}

fn print_latest_status(status: &LatestStatus) {
    println!("{}", render_latest_status_bar(status));
}

fn report_canonical_checkout_readonly(config: &RuntimeConfig) -> Vec<String> {
    let root = match std::env::current_dir() {
        Ok(root) => root,
        Err(error) => {
            return vec![format!("canonical_checkout_error={error}")];
        }
    };
    match inspect_canonical_checkout(&root, config) {
        Ok(report) => vec![canonical_checkout_status_line(&report)],
        Err(error) => vec![format!("canonical_checkout_error={error}")],
    }
}

fn enforce_canonical_checkout_before_write(
    config: &RuntimeConfig,
    command: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    match refresh_canonical_checkout_before_write(
        &root,
        config,
        CanonicalCheckoutRefreshMode::Apply,
    ) {
        Ok(refresh) => {
            println!("{}", canonical_checkout_refresh_status_line(&refresh));
            println!("{}", canonical_checkout_status_line(&refresh.checkout));
            for line in canonical_checkout_warning_lines(&refresh.checkout) {
                println!("{command}_{line}");
            }
            Ok(())
        }
        Err(error) => {
            println!(
                "canonical_checkout_refresh=blocked command={} reason=\"{}\"",
                shell_quote_display(command),
                single_line(&error.to_string())
            );
            Err(error.into())
        }
    }
}

fn preview_canonical_checkout_before_dry_run(config: &RuntimeConfig, command: &str) {
    let Ok(root) = std::env::current_dir() else {
        println!(
            "canonical_checkout_refresh=blocked command={} reason=\"current directory unavailable\"",
            shell_quote_display(command)
        );
        return;
    };
    match refresh_canonical_checkout_before_write(
        &root,
        config,
        CanonicalCheckoutRefreshMode::DryRun,
    ) {
        Ok(refresh) => {
            println!("{}", canonical_checkout_refresh_status_line(&refresh));
            println!("{}", canonical_checkout_status_line(&refresh.checkout));
            for line in canonical_checkout_warning_lines(&refresh.checkout) {
                println!("{command}_{line}");
            }
        }
        Err(error) => {
            println!(
                "canonical_checkout_refresh=blocked command={} reason=\"{}\"",
                shell_quote_display(command),
                single_line(&error.to_string())
            );
        }
    }
}

fn preflight_canonical_checkout_for_write_mode(
    config: &RuntimeConfig,
    command: &str,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if write {
        enforce_canonical_checkout_before_write(config, command)
    } else {
        preview_canonical_checkout_before_dry_run(config, command);
        Ok(())
    }
}

fn run_loop(options: RunLoopOptions) -> Result<(), Box<dyn std::error::Error>> {
    let limit = options.iteration_limit();
    let mut iterations = 0usize;

    loop {
        if let Some(max) = limit {
            if iterations >= max {
                println!("run_loop=stopped reason=max_iterations iterations={iterations}");
                break;
            }
        }

        iterations += 1;
        warn_if_temporary_workflow_path(&options.workflow_path);
        let workflow = WorkflowDefinition::load(&options.workflow_path)?;
        let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
        config.validate()?;
        let max_concurrent = options.worker_limit(&config);
        if options.write {
            ensure_write_mode_main_agent_backend(&options.workflow_path, &config, "main loop")?;
        }
        preflight_canonical_checkout_for_write_mode(&config, "run_loop", options.write)?;
        let adapter = adapter_from_config(&config);
        let mut active_main_workers = 0usize;
        let mut recoverable_runtime_states = Vec::new();
        if options.write {
            let runtime_states = load_runtime_states(&config)?;
            let preflight = run_loop_resume_preflight_many(
                adapter.as_ref(),
                &config,
                &runtime_states,
                current_time_ms(),
                options.recover,
            )?;
            save_runtime_states(&config, &preflight.retained_states)?;
            active_main_workers = preflight.active_main_workers;
            recoverable_runtime_states = preflight.recoverable_states;
            if let Some(reason) = preflight.blocked {
                println!("run_loop=stopped reason=resume_preflight_blocked detail={reason}");
                break;
            }
        }
        let issues = run_with_progress_heartbeat(
            progress_spec_with_event_log(&config, "github_project_read")
                .backend(tracker_backend_label(&config))
                .next("main_queue_scan"),
            || adapter.list_queue_scan_issues(),
        )?;
        let orchestrator = Orchestrator::new(config.clone());
        let mut plan = orchestrator.plan_dispatch(issues.clone());
        plan.integration_gaps.extend(adapter.integration_gaps());
        plan.snapshot.integration_gaps = plan.integration_gaps.clone();
        plan.snapshot.event_log_path = Some(
            config
                .observability
                .logs_root
                .join("jade-symphony.jsonl")
                .display()
                .to_string(),
        );

        let available_slots = if options.write {
            max_concurrent.saturating_sub(active_main_workers)
        } else {
            max_concurrent
        };
        if options.write && available_slots == 0 {
            if let Some(delay_ms) = unbounded_loop_sleep_ms(limit, config.polling.interval_ms) {
                println!(
                    "run_loop_idle action=sleep reason=max_concurrent_reached active_workers={} max_concurrent={} delay_ms={delay_ms} iterations={iterations}",
                    active_main_workers, max_concurrent
                );
                thread::sleep(Duration::from_millis(delay_ms));
                continue;
            } else {
                println!(
                    "run_loop=stopped reason=max_concurrent_reached active_workers={} max_concurrent={}",
                    active_main_workers, max_concurrent
                );
                break;
            }
        }
        let worker_id = worker_identity(&config, WorkerLane::Main);
        let selected = select_main_run_loop_issues(
            if options.write && options.recover {
                &recoverable_runtime_states
            } else {
                &[]
            },
            &plan.selected,
            available_slots,
            &worker_id,
            &config,
        );

        let Some(issue) = selected.first().cloned() else {
            plan.snapshot.latest_status = Some(LatestStatus {
                lane: "main".into(),
                category: "idle".into(),
                action: "no_dispatchable_issue".into(),
                issue_identifier: None,
                issue_title: None,
                actor_label: Some(config.identity.actor_label.clone()),
                workspace: None,
                branch: None,
                session_id: None,
                next: Some("wait for Todo/Rework or stop".into()),
            });
            if options.display == DisplayMode::Tui {
                println!(
                    "{}",
                    render_run_loop_panel(RunLoopPanel {
                        snapshot: &plan.snapshot,
                        issue: None,
                        handoff: None,
                        actor_role: "Main Agent",
                        mode: if options.write { "write" } else { "dry-run" },
                        max_concurrent,
                        selected_count: 0,
                    })
                );
            } else {
                println!("{}", render_snapshot(&plan.snapshot));
            }
            match no_dispatch_action(limit, config.polling.interval_ms) {
                NoDispatchAction::Stop { reason } => {
                    println!("run_loop=stopped reason={reason} iterations={iterations}");
                    break;
                }
                NoDispatchAction::SleepAndContinue { delay_ms } => {
                    println!(
                        "run_loop_idle action=sleep delay_ms={delay_ms} iterations={iterations}"
                    );
                    thread::sleep(Duration::from_millis(delay_ms));
                    continue;
                }
            }
        };
        let dispatch_candidates = selected.clone();
        if options.write {
            let should_stop_iteration = run_loop_dispatch_write_candidates(
                &workflow,
                &config,
                dispatch_candidates,
                &options,
                options.recover,
                iterations,
                max_concurrent,
            )?;
            if should_stop_iteration {
                break;
            }
            continue;
        }

        let issue = hydrate_issue_for_evidence(adapter.as_ref(), issue, &issues)?;

        let decision = evaluate_issue_for_current_source(&config, &issue)?;
        if !decision.is_dispatchable() {
            if options.display == DisplayMode::Tui {
                println!(
                    "{}",
                    render_run_loop_panel(RunLoopPanel {
                        snapshot: &plan.snapshot,
                        issue: Some(&issue),
                        handoff: None,
                        actor_role: "Main Agent",
                        mode: if options.write { "write" } else { "dry-run" },
                        max_concurrent,
                        selected_count: selected.len(),
                    })
                );
            }
            handle_run_loop_gate_failure(adapter.as_ref(), &issue, &decision, &options, &config)?;
            continue;
        }

        print_latest_status(&latest_status_for_issue(
            &config,
            &issue,
            "main",
            if options.write { "running" } else { "waiting" },
            if options.write {
                "selected"
            } else {
                "dry_run_plan"
            },
            Some(if options.write {
                "claim or resume".into()
            } else {
                "would claim and hand off to Agent Review".into()
            }),
        ));
        println!(
            "run_loop_iteration={} issue={} title={:?} mode={} max_concurrent={} selected_count={}",
            iterations,
            issue.identifier,
            issue.title,
            if options.write { "write" } else { "dry-run" },
            max_concurrent,
            selected.len()
        );

        let handoff = match run_loop_handoff_plan(&config, &issue) {
            Ok(handoff) => handoff,
            Err(error) => {
                handle_run_loop_handoff_failure(
                    adapter.as_ref(),
                    &issue,
                    &error,
                    &options,
                    &config,
                )?;
                continue;
            }
        };

        if !options.write {
            for candidate in &selected {
                let claim = lane_claim_for_issue(
                    candidate,
                    WorkerLane::Main.claim_lane(),
                    LaneClaimActor::Codex,
                    LaneClaimSource::Loop,
                    project_text_field(candidate, WorkerLane::Main.claim_field()).as_deref(),
                )
                .with_worker(&worker_id);
                write_lane_claim_field(
                    &config,
                    adapter.as_ref(),
                    candidate,
                    WorkerLane::Main,
                    &claim,
                    false,
                )?;
            }
            print_latest_status(&LatestStatus {
                lane: "main".into(),
                category: "handoff".into(),
                action: "dry_run_handoff_plan".into(),
                issue_identifier: Some(issue.identifier.clone()),
                issue_title: Some(issue.title.clone()),
                actor_label: Some(config.identity.actor_label.clone()),
                workspace: Some(handoff.workspace_path.display().to_string()),
                branch: Some(handoff.branch_name.clone()),
                session_id: None,
                next: Some("Agent Review".into()),
            });
            if options.display == DisplayMode::Tui {
                println!(
                    "{}",
                    render_run_loop_panel(RunLoopPanel {
                        snapshot: &plan.snapshot,
                        issue: Some(&issue),
                        handoff: Some(&handoff),
                        actor_role: "Main Agent",
                        mode: "dry-run",
                        max_concurrent,
                        selected_count: selected.len(),
                    })
                );
            }
            print_run_loop_dry_run_actions(&issue, &handoff, &config)?;
            if let Some(delay_ms) = unbounded_loop_sleep_ms(limit, config.polling.interval_ms) {
                println!(
                    "run_loop_idle action=sleep reason=dry_run_would_repeat_without_mutation delay_ms={delay_ms} iterations={iterations}"
                );
                thread::sleep(Duration::from_millis(delay_ms));
            }
            continue;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunLoopWorkerOutcome {
    Completed,
    StopIteration,
}

fn run_loop_dispatch_write_candidates(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    selected: Vec<TrackerIssue>,
    options: &RunLoopOptions,
    recover: bool,
    iterations: usize,
    max_concurrent: usize,
) -> Result<bool, Box<dyn std::error::Error>> {
    let selected_count = selected.len();
    let worker_id = worker_identity(config, WorkerLane::Main);
    let mut handles = Vec::new();

    for (slot_index, issue) in selected.into_iter().enumerate() {
        print_run_loop_write_selection(
            config,
            &issue,
            iterations,
            max_concurrent,
            selected_count,
            slot_index,
        );
        let issue_ref = issue.identifier.clone();
        let workflow = workflow.clone();
        let config = config.clone();
        let options = options.clone();
        let worker_id = worker_id.clone();
        handles.push((
            issue_ref,
            thread::spawn(move || {
                let adapter = adapter_from_config(&config);
                run_loop_dispatch_write_candidate(
                    &workflow,
                    &config,
                    adapter.as_ref(),
                    issue,
                    recover,
                    &worker_id,
                    &options,
                )
                .map_err(|error| error.to_string())
            }),
        ));
    }

    let mut should_stop_iteration = false;
    let mut errors = Vec::new();
    for (issue_ref, handle) in handles {
        match handle.join() {
            Ok(Ok(RunLoopWorkerOutcome::Completed)) => {}
            Ok(Ok(RunLoopWorkerOutcome::StopIteration)) => should_stop_iteration = true,
            Ok(Err(error)) => errors.push(format!("{issue_ref}: {error}")),
            Err(_) => errors.push(format!("{issue_ref}: worker thread panicked")),
        }
    }

    if !errors.is_empty() {
        return Err(format!("run_loop concurrent dispatch failed: {}", errors.join("; ")).into());
    }

    Ok(should_stop_iteration)
}

fn print_run_loop_write_selection(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    iterations: usize,
    max_concurrent: usize,
    selected_count: usize,
    slot_index: usize,
) {
    print_latest_status(&latest_status_for_issue(
        config,
        issue,
        "main",
        "running",
        "selected",
        Some("claim or resume".into()),
    ));
    if slot_index == 0 {
        println!(
            "run_loop_iteration={} issue={} title={:?} mode=write max_concurrent={} selected_count={}",
            iterations, issue.identifier, issue.title, max_concurrent, selected_count
        );
    } else {
        println!(
            "run_loop_iteration={} issue={} title={:?} mode=write max_concurrent={} selected_count={} slot={}",
            iterations,
            issue.identifier,
            issue.title,
            max_concurrent,
            selected_count,
            slot_index + 1
        );
    }
    let smoke_gate = main_app_server_smoke_gate(config);
    println!(
        "run_loop_action=backend issue={} backend={} backend_source={} command={} approval_policy={} app_server_live_smoke_ready={} session_registry={}",
        issue.identifier,
        smoke_gate.backend,
        smoke_gate.backend_source,
        shell_quote_display(&smoke_gate.command),
        smoke_gate.approval_policy,
        smoke_gate.app_server_live_smoke_ready,
        session_registry_path(config).display()
    );
}

fn run_loop_dispatch_write_candidate(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: TrackerIssue,
    recover: bool,
    worker_id: &str,
    options: &RunLoopOptions,
) -> Result<RunLoopWorkerOutcome, Box<dyn std::error::Error>> {
    let latest = if recover && normalize_state(&issue.state) == "in progress" {
        issue.clone()
    } else {
        run_with_progress_heartbeat(
            progress_spec_with_event_log(config, "github_project_read")
                .issue(issue.identifier.clone())
                .backend(tracker_backend_label(config))
                .next("main_issue_read"),
            || adapter.get_issue(&issue.identifier),
        )?
        .ok_or_else(|| format!("issue disappeared before claim: {}", issue.identifier))?
    };
    let eligibility = pool_claim_eligibility(&latest, WorkerLane::Main, worker_id, config);
    if !eligibility.is_claimable() {
        println!(
            "run_loop_action=skip issue={} reason={}",
            latest.identifier,
            eligibility.skip_reason()
        );
        return Ok(RunLoopWorkerOutcome::Completed);
    }
    let latest_gate = evaluate_issue_for_current_source(config, &latest)?;
    if !latest_gate.is_dispatchable() {
        handle_run_loop_gate_failure(adapter, &latest, &latest_gate, options, config)?;
        return Ok(RunLoopWorkerOutcome::Completed);
    }

    let mut handoff = match run_loop_handoff_plan(config, &latest) {
        Ok(handoff) => handoff,
        Err(error) => {
            handle_run_loop_handoff_failure(adapter, &latest, &error, options, config)?;
            return Ok(RunLoopWorkerOutcome::Completed);
        }
    };

    let profile_login = selected_profile_github_login(config)?;
    let active_login = if live_github_tracker(config) && profile_login.is_none() {
        current_gh_login()?
    } else {
        None
    };
    match run_loop_assignee_ownership_decision(
        &latest,
        config,
        active_login.as_deref(),
        profile_login.as_deref(),
    ) {
        AssigneeOwnershipDecision::Allowed => {}
        AssigneeOwnershipDecision::Block { reason } => {
            let workpad = run_loop_assignee_ownership_workpad(&latest, &reason);
            let workpad_key = recovery_key(
                "main-assignee-ownership-workpad",
                &latest.identifier,
                &format!("{}|{}", latest.identifier, stable_recovery_hash(&workpad)),
            );
            upsert_workpad_with_recovery(
                adapter,
                &latest.identifier,
                Some(&latest),
                &workpad,
                &workpad_key,
            )?;
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "blocked",
                "assignee_ownership",
                Some("operator intervention".into()),
            ));
            println!(
                "run_loop_action=skip issue={} reason=assignee_ownership detail={}",
                latest.identifier, reason
            );
            return Ok(RunLoopWorkerOutcome::Completed);
        }
    }

    let existing_runtime_states = load_runtime_states(config)?;
    let existing_runtime_state =
        runtime_state_for_issue(&existing_runtime_states, &latest.identifier);
    if let Some(state) = existing_runtime_state {
        if let Some(active_issue) = &state.active_issue {
            println!(
                "run_loop_runtime_state action=loaded active_issue={} attempt={}",
                active_issue.identifier, state.attempt_count
            );
        }
        if recover {
            if let Some(evidence) =
                run_loop_apply_recovery_handoff(config, &latest, &mut handoff, state)?
            {
                println!(
                    "run_loop_action=recovery_handoff issue={} evidence={}",
                    latest.identifier, evidence
                );
            }
        }
    }

    let ownership = run_loop_runtime_ownership(&latest, config, &handoff)?;
    let claim_action = run_loop_claim_action(&latest, config);
    let main_claim = lane_claim_for_issue(
        &latest,
        WorkerLane::Main.claim_lane(),
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        project_text_field(&latest, WorkerLane::Main.claim_field()).as_deref(),
    )
    .with_worker(worker_id);
    if matches!(claim_action, RunLoopClaimAction::Resume) {
        if let RuntimeOwnershipDecision::Mismatched { reason, .. } =
            runtime_ownership_decision(latest.description.as_deref(), &ownership)
        {
            println!(
                "run_loop_action=skip issue={} reason=ownership_mismatch detail={reason}",
                latest.identifier
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "blocked",
                "ownership_mismatch",
                Some("inspect runtime owner".into()),
            ));
            return Ok(RunLoopWorkerOutcome::Completed);
        }
    }

    let event = match claim_action {
        RunLoopClaimAction::Claim => {
            write_lane_claim_field(
                config,
                adapter,
                &latest,
                WorkerLane::Main,
                &main_claim,
                true,
            )?;
            let state_outcome = set_state_with_recovery(
                adapter,
                &latest.identifier,
                Some(&latest),
                "in_progress",
                "state_change",
            )?;
            if state_outcome.should_record_audit() {
                append_tracker_mutation_audit(
                    config,
                    TrackerMutationAudit {
                        command: "main loop",
                        mutation_type: "state_change",
                        issue_ref: Some(&latest.identifier),
                        target: None,
                        from_state: Some(latest.state.clone()),
                        to_state: Some("in_progress".into()),
                        reason: "main worker claim",
                    },
                );
            }
            println!(
                "run_loop_action=claim issue={} target_state=in_progress outcome={}",
                latest.identifier,
                state_outcome.as_str()
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "running",
                "claimed",
                Some("write runtime ownership".into()),
            ));
            "Claimed"
        }
        RunLoopClaimAction::Resume => {
            write_lane_claim_field(
                config,
                adapter,
                &latest,
                WorkerLane::Main,
                &main_claim,
                true,
            )?;
            println!("run_loop_action=resume issue={}", latest.identifier);
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "running",
                "resumed",
                Some("continue backend work".into()),
            ));
            "Resumed"
        }
        RunLoopClaimAction::StopAndReplan { current_state } => {
            println!(
                "run_loop_action=skip issue={} reason=external_state_change current_state={:?}",
                latest.identifier, current_state
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "waiting",
                "external_state_change",
                Some("replan".into()),
            ));
            return Ok(RunLoopWorkerOutcome::Completed);
        }
    };
    let ownership_workpad = run_loop_ownership_workpad(&latest, &ownership, event, &main_claim);
    let ownership_key = recovery_key(
        "main-ownership-workpad",
        &latest.identifier,
        &format!(
            "{}|{}|{}",
            latest.identifier,
            main_claim.run,
            stable_recovery_hash(&ownership_workpad)
        ),
    );
    let ownership_outcome = upsert_workpad_with_recovery(
        adapter,
        &latest.identifier,
        Some(&latest),
        &ownership_workpad,
        &ownership_key,
    )?;
    if ownership_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "main loop",
                mutation_type: "workpad_write",
                issue_ref: Some(&latest.identifier),
                target: ownership.profile_id.clone(),
                from_state: Some(latest.state.clone()),
                to_state: None,
                reason: "runtime ownership evidence",
            },
        );
    }
    println!(
        "run_loop_action=ownership issue={} profile={} branch={} outcome={}",
        latest.identifier,
        ownership.profile_id.as_deref().unwrap_or("n/a"),
        ownership.branch_name,
        ownership_outcome.as_str()
    );

    let mut runtime_state = run_loop_runtime_state_for_issue(
        existing_runtime_state,
        &latest,
        config,
        event,
        &main_claim,
    );
    runtime_state.branch_name = Some(handoff.branch_name.clone());
    mark_runtime_state_updated(&mut runtime_state, current_time_ms());
    upsert_runtime_state(config, &runtime_state)?;
    println!(
        "run_loop_runtime_state action=saved issue={} event={event}",
        latest.identifier
    );

    let live_worktree = if run_loop_live_handoff_enabled(config) {
        let runner = ProcessHandoffCommandRunner;
        let repo_root = std::env::current_dir()?;
        let worktree = prepare_issue_worktree(&repo_root, &handoff, &runner)?;
        println!(
            "run_loop_action=worktree issue={} workspace={} branch={} created={}",
            latest.identifier,
            worktree.workspace_path.display(),
            worktree.branch_name,
            worktree.created
        );
        print_latest_status(&LatestStatus {
            lane: "main".into(),
            category: "running".into(),
            action: "worktree_ready".into(),
            issue_identifier: Some(latest.identifier.clone()),
            issue_title: Some(latest.title.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            workspace: Some(worktree.workspace_path.display().to_string()),
            branch: Some(worktree.branch_name.clone()),
            session_id: runtime_state.backend_session_id.clone(),
            next: Some("run backend".into()),
        });
        Some(worktree)
    } else {
        None
    };

    let mut session_reconciliation =
        reconcile_pending_main_session(config, &latest, &handoff, &runtime_state)?;
    if let Some(MainSessionReconciliation::Active {
        status,
        source,
        evidence,
    }) = &session_reconciliation
    {
        let recover_active = recover && main_session_active_recoverable(status, evidence);
        append_runtime_supervision_event(
            config,
            Some(&runtime_state),
            if recover_active {
                "MainSessionRecovering"
            } else {
                "MainSessionStillActive"
            },
            &format!(
                "issue={} status={} source={} evidence={}",
                latest.identifier, status, source, evidence
            ),
        )?;
        println!(
            "run_loop_action=session_observed issue={} status={} source={} evidence={:?}",
            latest.identifier, status, source, evidence
        );
        if recover_active {
            println!(
                "run_loop_action=recover issue={} status={} source={} evidence={:?}",
                latest.identifier, status, source, evidence
            );
            session_reconciliation = None;
        } else {
            print_latest_status(&LatestStatus {
                lane: "main".into(),
                category: status.clone(),
                action: "session_observed".into(),
                issue_identifier: Some(latest.identifier.clone()),
                issue_title: Some(latest.title.clone()),
                actor_label: Some(config.identity.actor_label.clone()),
                workspace: runtime_state
                    .workspace_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                branch: runtime_state.branch_name.clone(),
                session_id: runtime_state.backend_session_id.clone(),
                next: runtime_state.backend_attach_command.clone(),
            });
            return Ok(RunLoopWorkerOutcome::StopIteration);
        }
    }
    if let Some(MainSessionReconciliation::Active {
        status,
        source: _,
        evidence: _,
    }) = &session_reconciliation
    {
        print_latest_status(&LatestStatus {
            lane: "main".into(),
            category: status.clone(),
            action: "session_observed".into(),
            issue_identifier: Some(latest.identifier.clone()),
            issue_title: Some(latest.title.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            workspace: runtime_state
                .workspace_path
                .as_ref()
                .map(|path| path.display().to_string()),
            branch: runtime_state.branch_name.clone(),
            session_id: runtime_state.backend_session_id.clone(),
            next: runtime_state.backend_attach_command.clone(),
        });
        return Ok(RunLoopWorkerOutcome::StopIteration);
    }

    print_latest_status(&latest_status_for_issue(
        config,
        &latest,
        "main",
        if session_reconciliation.is_some() {
            "reconciling"
        } else {
            "running"
        },
        if session_reconciliation.is_some() {
            "session_terminal"
        } else {
            "backend"
        },
        Some("save result".into()),
    ));
    let mut result = match session_reconciliation {
        Some(MainSessionReconciliation::Terminal(result)) => *result,
        Some(MainSessionReconciliation::Active { .. }) => unreachable!(),
        None => execute_issue_once_with_workspace_key(
            workflow,
            config,
            &latest,
            &handoff.workspace_key,
            runtime_state.attempt_count,
            Some(&main_claim),
        )?,
    };
    if result.success {
        if let Some(worktree) = live_worktree {
            let runner = ProcessHandoffCommandRunner;
            if result.backend == "codex" {
                let commit_message = format!(
                    "Implement {}: {}",
                    latest.identifier,
                    latest.title.replace(['\n', '\r'], " ")
                );
                match commit_issue_worktree_changes(&handoff, &runner, &commit_message) {
                    Ok(commit) => {
                        println!(
                            "run_loop_action=commit issue={} committed={} hash={}",
                            latest.identifier,
                            commit.committed,
                            commit.commit_hash.as_deref().unwrap_or("n/a")
                        );
                    }
                    Err(error) => {
                        result.success = false;
                        result.message = format!("handoff commit failed: {error}");
                    }
                }
            }
            let verification = if result.success {
                run_handoff_verification(&handoff.workspace_path, config)
            } else {
                HandoffVerification {
                    success: false,
                    summary: result.message.clone(),
                }
            };
            println!(
                "run_loop_action=verify issue={} success={} summary={}",
                latest.identifier, verification.success, verification.summary
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                if verification.success {
                    "handoff"
                } else {
                    "failed"
                },
                "verify",
                Some(if verification.success {
                    "publish PR".into()
                } else {
                    "record failure".into()
                }),
            ));
            result.handoff_verification = Some(verification.summary.clone());
            if verification.success {
                match publish_issue_pull_request(&handoff, &runner) {
                    Ok(publication) => {
                        println!(
                            "run_loop_action=pr issue={} url={} created={}",
                            latest.identifier, publication.pr_url, publication.pr_created
                        );
                        print_latest_status(&LatestStatus {
                            lane: "main".into(),
                            category: "handoff".into(),
                            action: "pr_ready".into(),
                            issue_identifier: Some(latest.identifier.clone()),
                            issue_title: Some(latest.title.clone()),
                            actor_label: Some(config.identity.actor_label.clone()),
                            workspace: Some(worktree.workspace_path.display().to_string()),
                            branch: Some(worktree.branch_name.clone()),
                            session_id: result.session_id.clone(),
                            next: Some("link PR".into()),
                        });
                        result.live_handoff = Some(RunLoopLiveHandoff {
                            worktree,
                            publication,
                            verification: verification.summary,
                            project_pr_link_verified: None,
                            pull_request_ready: None,
                        });
                    }
                    Err(error) => {
                        result.success = false;
                        result.message = format!("handoff publication failed: {error}");
                    }
                }
            } else {
                result.success = false;
                result.message = format!("handoff verification failed: {}", verification.summary);
            }
        }
        if result.success {
            let linked = apply_live_handoff_pr_link(adapter, &latest.identifier, &mut result);
            if linked {
                append_tracker_mutation_audit(
                    config,
                    TrackerMutationAudit {
                        command: "main loop",
                        mutation_type: "pr_link",
                        issue_ref: Some(&latest.identifier),
                        target: result
                            .live_handoff
                            .as_ref()
                            .map(|handoff| handoff.publication.pr_url.clone()),
                        from_state: Some(latest.state.clone()),
                        to_state: None,
                        reason: "live handoff PR link",
                    },
                );
                println!(
                    "run_loop_action=link_pr issue={} evidence=live_handoff",
                    latest.identifier
                );
            }
        }
        if result.success {
            if let Some(handoff) = result.live_handoff.as_mut() {
                match ensure_pull_request_ready(
                    &handoff.publication.pr_url,
                    &ProcessHandoffCommandRunner,
                    &handoff.worktree.workspace_path,
                ) {
                    Ok(ready) => {
                        println!(
                            "run_loop_action=pr_ready issue={} url={} was_draft={} marked_ready={}",
                            latest.identifier, ready.pr_url, ready.was_draft, ready.marked_ready
                        );
                        handoff.pull_request_ready = Some(ready);
                    }
                    Err(error) => {
                        result.success = false;
                        result.message = format!("handoff PR ready check failed: {error}");
                        println!(
                        "run_loop_action=blocked issue={} reason=pr_ready_check_failed error={}",
                        latest.identifier, error
                    );
                    }
                }
            }
        }
    }
    runtime_state = run_loop_runtime_state_with_result(runtime_state, &result);
    mark_runtime_state_updated(&mut runtime_state, current_time_ms());
    upsert_runtime_state(config, &runtime_state)?;
    println!(
        "run_loop_runtime_state action=updated issue={} event={}",
        latest.identifier,
        runtime_state.last_event.as_deref().unwrap_or("unknown")
    );

    let workpad = run_loop_handoff_workpad(&latest, &result, &handoff, Some(&ownership));
    let handoff_key = recovery_key(
        "main-handoff-workpad",
        &latest.identifier,
        &format!(
            "{}|{}|{}",
            latest.identifier,
            main_claim.run,
            stable_recovery_hash(&workpad)
        ),
    );
    let handoff_outcome = upsert_workpad_with_recovery(
        adapter,
        &latest.identifier,
        Some(&latest),
        &workpad,
        &handoff_key,
    )?;
    if handoff_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "main loop",
                mutation_type: "workpad_write",
                issue_ref: Some(&latest.identifier),
                target: result
                    .live_handoff
                    .as_ref()
                    .map(|handoff| handoff.publication.pr_url.clone()),
                from_state: Some(latest.state.clone()),
                to_state: None,
                reason: "main worker handoff evidence",
            },
        );
    }

    if result.pending_session {
        append_runtime_supervision_event(
            config,
            Some(&runtime_state),
            "TmuxSessionRunning",
            &format!(
                "issue={} session={} attach_command={} log_path={}",
                latest.identifier,
                result.session_id.as_deref().unwrap_or("n/a"),
                result.backend_attach_command.as_deref().unwrap_or("n/a"),
                result
                    .backend_log_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "n/a".into())
            ),
        )?;
        println!(
        "run_loop_action=session_started issue={} backend={} session={} attach_command=\"{}\" log_path={}",
        latest.identifier,
        result.backend,
        result.session_id.as_deref().unwrap_or("n/a"),
        result
            .backend_attach_command
            .as_deref()
            .unwrap_or("n/a"),
        result
            .backend_log_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "n/a".into())
    );
        print_latest_status(&LatestStatus {
            lane: "main".into(),
            category: "running".into(),
            action: "session_started".into(),
            issue_identifier: Some(latest.identifier.clone()),
            issue_title: Some(latest.title.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            workspace: Some(result.workspace_path.display().to_string()),
            branch: runtime_state.branch_name.clone(),
            session_id: result.session_id.clone(),
            next: result.backend_attach_command.clone(),
        });
        return Ok(RunLoopWorkerOutcome::Completed);
    }

    if result.success {
        if !transition_allowed_for_main_agent("agent_review") {
            return Err("main implementation agent cannot set requested review state".into());
        }
        let evidence =
            run_loop_agent_review_handoff_evidence(&latest, &result, &handoff, Some(&workpad));
        let handoff_report = evaluate_agent_review_handoff(&evidence);
        let handoff_workpad =
            render_agent_review_handoff_workpad(&latest, &evidence, &handoff_report);
        let review_handoff_key = recovery_key(
            "agent-review-handoff-workpad",
            &latest.identifier,
            &format!(
                "{}|{}|{}",
                latest.identifier,
                main_claim.run,
                stable_recovery_hash(&handoff_workpad)
            ),
        );
        let review_handoff_outcome = upsert_workpad_with_recovery(
            adapter,
            &latest.identifier,
            Some(&latest),
            &handoff_workpad,
            &review_handoff_key,
        )?;
        if review_handoff_outcome.should_record_audit() {
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "main loop",
                    mutation_type: "workpad_write",
                    issue_ref: Some(&latest.identifier),
                    target: result
                        .live_handoff
                        .as_ref()
                        .map(|handoff| handoff.publication.pr_url.clone()),
                    from_state: Some(latest.state.clone()),
                    to_state: Some("agent_review".into()),
                    reason: "agent review handoff evidence",
                },
            );
        }
        if !handoff_report.is_ready() {
            runtime_state = run_loop_runtime_state_with_transition(
                runtime_state,
                Some(latest.state.clone()),
                "need_human_input",
                "agent review handoff invariant failed",
            );
            upsert_runtime_state(config, &runtime_state)?;
            write_lane_claim_state(
                config,
                adapter,
                &latest,
                WorkerLane::Main,
                &main_claim,
                LaneClaimState::Failed,
            )?;
            let state_outcome = set_state_with_recovery(
                adapter,
                &latest.identifier,
                Some(&latest),
                "need_human_input",
                "state_change",
            )?;
            if state_outcome.should_record_audit() {
                append_tracker_mutation_audit(
                    config,
                    TrackerMutationAudit {
                        command: "main loop",
                        mutation_type: "state_change",
                        issue_ref: Some(&latest.identifier),
                        target: None,
                        from_state: Some(latest.state.clone()),
                        to_state: Some("need_human_input".into()),
                        reason: "agent review handoff invariant failed",
                    },
                );
            }
            remove_runtime_state_for_issue(config, &latest.identifier)?;
            println!(
            "run_loop_action=blocked issue={} target_state=need_human_input reason=handoff_invariant_failed",
            latest.identifier
        );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "blocked",
                "handoff_invariant_failed",
                Some("Need Human Input".into()),
            ));
            return Ok(RunLoopWorkerOutcome::Completed);
        }
        runtime_state = run_loop_runtime_state_with_transition(
            runtime_state,
            Some(latest.state.clone()),
            "agent_review",
            "main agent completed",
        );
        mark_runtime_state_updated(&mut runtime_state, current_time_ms());
        upsert_runtime_state(config, &runtime_state)?;
        write_lane_claim_state(
            config,
            adapter,
            &latest,
            WorkerLane::Main,
            &main_claim,
            LaneClaimState::Done,
        )?;
        let state_outcome = set_state_with_recovery(
            adapter,
            &latest.identifier,
            Some(&latest),
            "agent_review",
            "state_change",
        )?;
        reconcile_main_handoff_runtime_state(config, &latest.identifier, "agent_review")?;
        if state_outcome.should_record_audit() {
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "main loop",
                    mutation_type: "state_change",
                    issue_ref: Some(&latest.identifier),
                    target: result
                        .live_handoff
                        .as_ref()
                        .map(|handoff| handoff.publication.pr_url.clone()),
                    from_state: Some(latest.state.clone()),
                    to_state: Some("agent_review".into()),
                    reason: "main agent completed",
                },
            );
        }
        remove_runtime_state_for_issue(config, &latest.identifier)?;
        println!(
            "run_loop_action=handoff issue={} target_state=agent_review",
            latest.identifier
        );
        print_latest_status(&latest_status_for_issue(
            config,
            &latest,
            "main",
            "handoff",
            "agent_review",
            Some("Review Agent".into()),
        ));
    } else {
        let retry_delay_ms =
            Orchestrator::new(config.clone()).retry_delay_ms(runtime_state.attempt_count, false);
        if let Some(pause) = &result.usage_limit_pause {
            record_runtime_retry(
                &mut runtime_state,
                current_time_ms(),
                retry_delay_ms,
                format!("usage-limit pause: {}", pause.evidence),
            );
            upsert_runtime_state(config, &runtime_state)?;
            let pause_workpad =
                run_loop_usage_limit_pause_workpad(&latest, &result, pause, retry_delay_ms);
            let pause_key = recovery_key(
                "main-usage-limit-workpad",
                &latest.identifier,
                &format!(
                    "{}|{}|{}",
                    latest.identifier,
                    main_claim.run,
                    stable_recovery_hash(&pause_workpad)
                ),
            );
            let pause_outcome = upsert_workpad_with_recovery(
                adapter,
                &latest.identifier,
                Some(&latest),
                &pause_workpad,
                &pause_key,
            )?;
            if pause_outcome.should_record_audit() {
                append_tracker_mutation_audit(
                    config,
                    TrackerMutationAudit {
                        command: "main loop",
                        mutation_type: "workpad_write",
                        issue_ref: Some(&latest.identifier),
                        target: Some(pause.classifier.clone()),
                        from_state: Some(latest.state.clone()),
                        to_state: None,
                        reason: "usage-limit pause evidence",
                    },
                );
            }
            append_runtime_supervision_event(
                config,
                Some(&runtime_state),
                "UsageLimitPaused",
                &format!(
                    "issue={} classifier={} due_in_ms={} evidence={}",
                    latest.identifier, pause.classifier, retry_delay_ms, pause.evidence
                ),
            )?;
            println!(
                "run_loop_action=usage_limit_paused issue={} classifier={} due_in_ms={}",
                latest.identifier, pause.classifier, retry_delay_ms
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "retrying",
                "usage_limit_paused",
                Some(format!("retry in {retry_delay_ms}ms")),
            ));
            return Ok(RunLoopWorkerOutcome::StopIteration);
        }
        if result.message.contains("handoff PR link") {
            runtime_state = run_loop_runtime_state_with_transition(
                runtime_state,
                Some(latest.state.clone()),
                "need_human_input",
                "handoff PR linkage invariant failed",
            );
            mark_runtime_state_updated(&mut runtime_state, current_time_ms());
            upsert_runtime_state(config, &runtime_state)?;
            write_lane_claim_state(
                config,
                adapter,
                &latest,
                WorkerLane::Main,
                &main_claim,
                LaneClaimState::Failed,
            )?;
            let state_outcome = set_state_with_recovery(
                adapter,
                &latest.identifier,
                Some(&latest),
                "need_human_input",
                "state_change",
            )?;
            if state_outcome.should_record_audit() {
                append_tracker_mutation_audit(
                    config,
                    TrackerMutationAudit {
                        command: "main loop",
                        mutation_type: "state_change",
                        issue_ref: Some(&latest.identifier),
                        target: result
                            .live_handoff
                            .as_ref()
                            .map(|handoff| handoff.publication.pr_url.clone()),
                        from_state: Some(latest.state.clone()),
                        to_state: Some("need_human_input".into()),
                        reason: "handoff PR linkage invariant failed",
                    },
                );
            }
            remove_runtime_state_for_issue(config, &latest.identifier)?;
            println!(
            "run_loop_action=blocked issue={} target_state=need_human_input reason=handoff_pr_linkage_invariant_failed",
            latest.identifier
        );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "blocked",
                "handoff_pr_linkage",
                Some("Need Human Input".into()),
            ));
            return Ok(RunLoopWorkerOutcome::Completed);
        }
        if runtime_state.attempt_count < config.agent.max_turns {
            record_runtime_retry(
                &mut runtime_state,
                current_time_ms(),
                retry_delay_ms,
                result.message.clone(),
            );
            upsert_runtime_state(config, &runtime_state)?;
            append_runtime_supervision_event(
                config,
                Some(&runtime_state),
                "RetryScheduled",
                &format!(
                    "issue={} attempt={} due_in_ms={} error={}",
                    latest.identifier, runtime_state.attempt_count, retry_delay_ms, result.message
                ),
            )?;
            println!(
                "run_loop_action=retry_scheduled issue={} attempt={} due_in_ms={}",
                latest.identifier, runtime_state.attempt_count, retry_delay_ms
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "retrying",
                "retry_scheduled",
                Some(format!("retry in {retry_delay_ms}ms")),
            ));
            return Ok(RunLoopWorkerOutcome::StopIteration);
        } else {
            runtime_state = run_loop_runtime_state_with_transition(
                runtime_state,
                Some(latest.state.clone()),
                "need_human_input",
                "backend run failed after retry limit",
            );
            mark_runtime_state_updated(&mut runtime_state, current_time_ms());
            upsert_runtime_state(config, &runtime_state)?;
            let state_outcome = set_state_with_recovery(
                adapter,
                &latest.identifier,
                Some(&latest),
                "need_human_input",
                "state_change",
            )?;
            if state_outcome.should_record_audit() {
                append_tracker_mutation_audit(
                    config,
                    TrackerMutationAudit {
                        command: "main loop",
                        mutation_type: "state_change",
                        issue_ref: Some(&latest.identifier),
                        target: None,
                        from_state: Some(latest.state.clone()),
                        to_state: Some("need_human_input".into()),
                        reason: "backend run failed after retry limit",
                    },
                );
            }
            remove_runtime_state_for_issue(config, &latest.identifier)?;
            println!(
                "run_loop_action=blocked issue={} target_state=need_human_input",
                latest.identifier
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "failed",
                "need_human_input",
                Some("operator repair".into()),
            ));
        }
    }

    Ok(RunLoopWorkerOutcome::Completed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoDispatchAction {
    Stop { reason: &'static str },
    SleepAndContinue { delay_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RunLoopClaimAction {
    Claim,
    Resume,
    StopAndReplan { current_state: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerLane {
    Main,
    Merging,
}

impl WorkerLane {
    fn claim_field(self) -> &'static str {
        match self {
            Self::Main => "Main Agent",
            Self::Merging => "Merging Agent",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Merging => "merging",
        }
    }

    fn claim_lane(self) -> LaneClaimLane {
        match self {
            Self::Main => LaneClaimLane::Main,
            Self::Merging => LaneClaimLane::Merge,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PoolClaimEligibility {
    Claimable,
    OwnedBySelf,
    ClaimedByOther { owner: String },
    WrongLaneState { state: String },
    ParentNativeSubissuesIncomplete { reason: String },
}

impl PoolClaimEligibility {
    fn is_claimable(&self) -> bool {
        matches!(self, Self::Claimable | Self::OwnedBySelf)
    }

    fn skip_reason(&self) -> String {
        match self {
            Self::Claimable | Self::OwnedBySelf => "claimable".into(),
            Self::ClaimedByOther { owner } => format!("claimed_by_other:{owner}"),
            Self::WrongLaneState { state } => format!("wrong_lane_state:{state}"),
            Self::ParentNativeSubissuesIncomplete { reason } => reason.clone(),
        }
    }
}

fn worker_identity(config: &RuntimeConfig, lane: WorkerLane) -> String {
    let label = config.identity.actor_label.trim();
    if label.is_empty() {
        format!("jade-symphony-{}", lane.label())
    } else {
        label.to_string()
    }
}

fn project_text_field(issue: &TrackerIssue, name: &str) -> Option<String> {
    issue
        .project_fields
        .get(name)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn pool_claim_eligibility(
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

fn select_pool_worker_issues(
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

fn select_main_run_loop_issues(
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

#[derive(Debug, Clone, PartialEq)]
struct MergeWorkerSelection {
    issue: TrackerIssue,
    recovery_reason: Option<String>,
}

fn select_merge_worker_issues(
    issues: &[TrackerIssue],
    worker_id: &str,
    pool: usize,
    config: &RuntimeConfig,
    recover: bool,
) -> Vec<MergeWorkerSelection> {
    let limit = pool.max(1);
    let mut selected = Vec::new();

    if recover {
        let mut recovery_candidates = issues
            .iter()
            .filter_map(|issue| {
                merge_recovery_reason(issue, worker_id, config).map(|reason| MergeWorkerSelection {
                    issue: issue.clone(),
                    recovery_reason: Some(reason),
                })
            })
            .collect::<Vec<_>>();
        recovery_candidates.sort_by_key(|candidate| candidate.issue.priority.unwrap_or(i64::MAX));
        for candidate in recovery_candidates {
            if selected.len() >= limit {
                break;
            }
            selected.push(candidate);
        }
    }

    let remaining = limit.saturating_sub(selected.len());
    if remaining > 0 {
        for issue in
            select_pool_worker_issues(issues, WorkerLane::Merging, worker_id, remaining, config)
        {
            if selected.iter().any(|candidate: &MergeWorkerSelection| {
                candidate.issue.identifier == issue.identifier
            }) {
                continue;
            }
            selected.push(MergeWorkerSelection {
                issue,
                recovery_reason: None,
            });
        }
    }

    selected
}

fn merge_recovery_reason(
    issue: &TrackerIssue,
    worker_id: &str,
    config: &RuntimeConfig,
) -> Option<String> {
    let normalized_state = issue.normalized_state();
    if normalized_state != normalize_state(&config.tracker.state_map.merging) {
        return None;
    }

    let owner = project_text_field(issue, WorkerLane::Merging.claim_field())?;
    let claim = LaneClaim::parse(&owner).ok()?;
    if claim.lane != LaneClaimLane::Merge
        || claim.issue != issue.identifier
        || claim.state != LaneClaimState::Active
        || !matches!(claim.source, LaneClaimSource::Loop | LaneClaimSource::Goal)
        || claim.worker.as_deref() == Some(worker_id)
    {
        return None;
    }

    Some(format!(
        "recover_active_merge_claim previous_worker={} run={} source={}",
        claim.worker.as_deref().unwrap_or("unknown"),
        claim.run,
        claim.source.as_str()
    ))
}

fn write_lane_claim_field(
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

fn write_lane_claim_state(
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResumePreflightAction {
    Continue,
    ArchiveStale {
        issue_identifier: String,
        tracker_state: String,
        archive_reason: String,
    },
    RetryLater {
        issue_identifier: String,
        retry: RuntimeRetryState,
        due_in_ms: u64,
    },
    Stalled {
        issue_identifier: String,
        stall: RuntimeStallState,
    },
    Block {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct RuntimePreflightSummary {
    retained_states: Vec<RuntimeState>,
    active_main_workers: usize,
    recoverable_states: Vec<RuntimeRecoveryCandidate>,
    blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct RuntimeRecoveryCandidate {
    state: RuntimeState,
    issue: TrackerIssue,
    reason: String,
}

#[derive(Debug, Clone, PartialEq)]
struct ActiveRuntimeSessionProbe {
    state: RuntimeState,
    session_name: String,
    probe: SessionStatusProbe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeWorkspaceStatus {
    Absent,
    Clean(PathBuf),
    Dirty(PathBuf),
    Unknown { path: PathBuf, reason: String },
}

fn run_loop_claim_action(issue: &TrackerIssue, config: &RuntimeConfig) -> RunLoopClaimAction {
    match claim_decision(issue, config) {
        ClaimDecision::Claimable => RunLoopClaimAction::Claim,
        ClaimDecision::AlreadyInProgress => RunLoopClaimAction::Resume,
        ClaimDecision::StopAndReplan { current_state } => {
            RunLoopClaimAction::StopAndReplan { current_state }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AssigneeOwnershipDecision {
    Allowed,
    Block { reason: String },
}

fn run_loop_assignee_ownership_decision(
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

fn live_github_tracker(config: &RuntimeConfig) -> bool {
    config.tracker.kind == "github_project_v2" && config.tracker.fixture_path.is_none()
}

fn append_canonical_checkout_gap(config: &RuntimeConfig, gaps: &mut Vec<String>) {
    if !live_github_tracker(config) {
        return;
    }
    let Ok(current_dir) = std::env::current_dir() else {
        gaps.push("canonical_checkout_blocked: current directory is unavailable".into());
        return;
    };
    if let Some(reason) = canonical_checkout_report(&current_dir).blocker() {
        gaps.push(format!("canonical_checkout_blocked: {reason}"));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CanonicalCheckoutReport {
    Ready,
    Blocked { reason: String },
}

impl CanonicalCheckoutReport {
    fn blocker(&self) -> Option<&str> {
        match self {
            Self::Ready => None,
            Self::Blocked { reason } => Some(reason.as_str()),
        }
    }
}

fn canonical_checkout_report(path: &Path) -> CanonicalCheckoutReport {
    let branch = match git_stdout(path, &["branch", "--show-current"]) {
        Ok(branch) if !branch.trim().is_empty() => branch.trim().to_string(),
        Ok(_) => {
            return CanonicalCheckoutReport::Blocked {
                reason: "HEAD is detached".into(),
            }
        }
        Err(error) => {
            return CanonicalCheckoutReport::Blocked {
                reason: format!("git branch check failed: {error}"),
            }
        }
    };
    if branch != "main" {
        return CanonicalCheckoutReport::Blocked {
            reason: format!("current branch is {branch:?}, expected \"main\""),
        };
    }

    if let Err(error) = git_status(path, &["fetch", "--quiet", "origin", "main"]) {
        return CanonicalCheckoutReport::Blocked {
            reason: format!("git fetch origin main failed: {error}"),
        };
    }

    let head = match git_stdout(path, &["rev-parse", "HEAD"]) {
        Ok(value) => value.trim().to_string(),
        Err(error) => {
            return CanonicalCheckoutReport::Blocked {
                reason: format!("cannot read HEAD: {error}"),
            }
        }
    };
    let origin_main = match git_stdout(path, &["rev-parse", "origin/main"]) {
        Ok(value) => value.trim().to_string(),
        Err(error) => {
            return CanonicalCheckoutReport::Blocked {
                reason: format!("cannot read origin/main: {error}"),
            }
        }
    };
    if head != origin_main {
        return CanonicalCheckoutReport::Blocked {
            reason: "local main does not exactly match origin/main".into(),
        };
    }

    CanonicalCheckoutReport::Ready
}

fn git_stdout(path: &Path, args: &[&str]) -> Result<String, String> {
    let output = ProcessCommand::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!(
                "git {:?} exited with status {:?}",
                args,
                output.status.code()
            )
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_status(path: &Path, args: &[&str]) -> Result<(), String> {
    git_stdout(path, args).map(|_| ())
}

fn normalized_login(value: &str) -> String {
    value.trim().trim_start_matches('@').to_ascii_lowercase()
}

fn selected_profile_github_login(
    config: &RuntimeConfig,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    Ok(
        selected_execution_profile(&config.profiles)?.and_then(|profile| {
            profile
                .env
                .get("GITHUB_LOGIN")
                .cloned()
                .or_else(|| profile.env.get("GH_LOGIN").cloned())
                .or_else(|| profile.env.get("JADE_GITHUB_LOGIN").cloned())
        }),
    )
}

fn current_gh_login() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let output = ProcessCommand::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    if !output.status.success() {
        return Ok(None);
    }

    let login = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!login.is_empty()).then_some(login))
}

fn run_loop_resume_preflight(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    config: &RuntimeConfig,
    state: Option<&RuntimeState>,
    now_ms: u64,
) -> Result<ResumePreflightAction, Box<dyn std::error::Error>> {
    let Some(state) = state else {
        return Ok(ResumePreflightAction::Continue);
    };
    let Some(active_issue) = state.active_issue.as_ref() else {
        return Ok(ResumePreflightAction::Continue);
    };

    let Some(issue) = adapter.get_issue(&active_issue.identifier)? else {
        return Ok(ResumePreflightAction::Block {
            reason: format!(
                "runtime state references missing issue {}",
                active_issue.identifier
            ),
        });
    };
    let normalized_state = normalize_state(&issue.state);

    if normalized_state != "in progress" {
        return stale_runtime_state_action(state, &issue, &normalized_state, config);
    }

    if let Some(retry) = state.retry.clone() {
        let due_in_ms = retry.due_in_ms(now_ms);
        if due_in_ms > 0 {
            return Ok(ResumePreflightAction::RetryLater {
                issue_identifier: active_issue.identifier.clone(),
                retry,
                due_in_ms,
            });
        }
        return Ok(ResumePreflightAction::Continue);
    }

    if let Some(stall) = detect_runtime_stall(state, now_ms, config.codex.stall_timeout_ms) {
        return Ok(ResumePreflightAction::Stalled {
            issue_identifier: active_issue.identifier.clone(),
            stall,
        });
    }

    Ok(ResumePreflightAction::Continue)
}

fn run_loop_resume_preflight_many(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    config: &RuntimeConfig,
    states: &[RuntimeState],
    now_ms: u64,
    recover: bool,
) -> Result<RuntimePreflightSummary, Box<dyn std::error::Error>> {
    let mut retained_states = Vec::new();
    let mut active_main_workers = 0usize;
    let mut recoverable_states = Vec::new();
    let mut blocked = None;

    for state in states {
        if !runtime_state_is_main_lane(state) {
            retained_states.push(state.clone());
            continue;
        }

        if recover {
            let in_progress_issue = match runtime_state_in_progress_issue(adapter, state) {
                Ok(issue) => issue,
                Err(error) => {
                    let issue_identifier = state
                        .active_issue
                        .as_ref()
                        .map(|issue| issue.identifier.as_str())
                        .unwrap_or("unknown");
                    let reason = format!(
                        "tracker_read_failed: {}",
                        compact_evidence(&error.to_string())
                    );
                    append_runtime_supervision_event(
                        config,
                        Some(state),
                        "RuntimeRecoverReadSkipped",
                        &format!("issue={issue_identifier} reason={reason}"),
                    )?;
                    println!(
                        "run_loop_resume_preflight action=recover_read_skipped issue={} reason={}",
                        issue_identifier, reason
                    );
                    retained_states.push(state.clone());
                    continue;
                }
            };
            if let Some(issue) = in_progress_issue {
                if let Some(active_session) =
                    active_runtime_session_for_issue(config, state, now_ms)?
                {
                    if session_status_counts_as_active_worker(&active_session.probe.status) {
                        let issue_identifier = state
                            .active_issue
                            .as_ref()
                            .map(|issue| issue.identifier.as_str())
                            .unwrap_or("unknown");
                        active_main_workers += 1;
                        append_runtime_supervision_event(
                            config,
                            Some(state),
                            "RuntimeRecoverSkippedActiveSession",
                            &format!(
                                "issue={issue_identifier} session={} status={} source={} evidence={}",
                                active_session.session_name,
                                active_session.probe.status.as_str(),
                                active_session.probe.source.as_str(),
                                compact_evidence(&active_session.probe.evidence)
                            ),
                        )?;
                        println!(
                            "run_loop_resume_preflight action=active_session issue={} session={} status={} source={} evidence={}",
                            issue_identifier,
                            active_session.session_name,
                            active_session.probe.status.as_str(),
                            active_session.probe.source.as_str(),
                            compact_evidence(&active_session.probe.evidence)
                        );
                        retained_states.push(active_session.state);
                        continue;
                    }
                }
                if let Some(terminal_session) =
                    terminal_runtime_session_for_issue(config, state, now_ms)?
                {
                    let issue_identifier = state
                        .active_issue
                        .as_ref()
                        .map(|issue| issue.identifier.as_str())
                        .unwrap_or("unknown");
                    let reason = format!(
                        "registry_session_terminal session={} status={} source={} evidence={}",
                        terminal_session.session_name,
                        terminal_session.probe.status.as_str(),
                        terminal_session.probe.source.as_str(),
                        compact_evidence(&terminal_session.probe.evidence)
                    );
                    append_runtime_supervision_event(
                        config,
                        Some(&terminal_session.state),
                        "RuntimeRecoverableTerminalSession",
                        &format!("issue={issue_identifier} reason={reason}"),
                    )?;
                    println!(
                        "run_loop_resume_preflight action=recoverable_terminal_session issue={} session={} status={} source={}",
                        issue_identifier,
                        terminal_session.session_name,
                        terminal_session.probe.status.as_str(),
                        terminal_session.probe.source.as_str()
                    );
                    recoverable_states.push(RuntimeRecoveryCandidate {
                        state: terminal_session.state.clone(),
                        issue,
                        reason,
                    });
                    retained_states.push(terminal_session.state);
                    continue;
                }
                if let Some(reason) = runtime_recovery_reason(config, state, now_ms)? {
                    let issue_identifier = state
                        .active_issue
                        .as_ref()
                        .map(|issue| issue.identifier.as_str())
                        .unwrap_or("unknown");
                    append_runtime_supervision_event(
                        config,
                        Some(state),
                        "RuntimeRecoverable",
                        &format!("issue={issue_identifier} reason={reason}"),
                    )?;
                    println!(
                        "run_loop_resume_preflight action=recoverable issue={} reason={}",
                        issue_identifier, reason
                    );
                    recoverable_states.push(RuntimeRecoveryCandidate {
                        state: state.clone(),
                        issue,
                        reason,
                    });
                    retained_states.push(state.clone());
                    continue;
                }
            }
        }

        let action = run_loop_resume_preflight(adapter, config, Some(state), now_ms)?;
        match action {
            ResumePreflightAction::Continue => {
                if runtime_state_points_at_in_progress_issue(adapter, state)? {
                    active_main_workers += 1;
                }
                retained_states.push(state.clone());
            }
            ResumePreflightAction::ArchiveStale {
                issue_identifier,
                tracker_state,
                archive_reason,
            } => {
                let archive_path = archive_runtime_state(config, state, &archive_reason)?;
                append_runtime_supervision_event(
                    config,
                    Some(state),
                    "RuntimeStateArchived",
                    &format!(
                        "issue={issue_identifier} tracker_state={tracker_state} reason={archive_reason} archive_path={}",
                        archive_path.display()
                    ),
                )?;
                println!(
                    "run_loop_resume_preflight action=archive issue={} tracker_state={:?} reason={} archive_path={}",
                    issue_identifier,
                    tracker_state,
                    archive_reason,
                    archive_path.display()
                );
            }
            ResumePreflightAction::RetryLater {
                issue_identifier,
                retry,
                due_in_ms,
            } => {
                active_main_workers += 1;
                append_runtime_supervision_event(
                    config,
                    Some(state),
                    "RetryDeferred",
                    &format!(
                        "issue={issue_identifier} attempt={} due_in_ms={} error={}",
                        retry.attempt, due_in_ms, retry.error
                    ),
                )?;
                println!(
                    "run_loop_resume_preflight action=retry_backoff issue={} due_in_ms={} attempt={}",
                    issue_identifier, due_in_ms, retry.attempt
                );
                retained_states.push(state.clone());
            }
            ResumePreflightAction::Stalled {
                issue_identifier,
                stall,
            } => {
                if recover {
                    let Some(issue) = runtime_state_in_progress_issue(adapter, state)? else {
                        retained_states.push(state.clone());
                        continue;
                    };
                    let reason = format!(
                        "runtime_stalled stalled_for_ms={} reason={}",
                        stall.stalled_for_ms, stall.reason
                    );
                    append_runtime_supervision_event(
                        config,
                        Some(state),
                        "RuntimeRecoverable",
                        &format!("issue={issue_identifier} reason={reason}"),
                    )?;
                    println!(
                        "run_loop_resume_preflight action=recoverable issue={} stalled_for_ms={}",
                        issue_identifier, stall.stalled_for_ms
                    );
                    recoverable_states.push(RuntimeRecoveryCandidate {
                        state: state.clone(),
                        issue,
                        reason,
                    });
                    retained_states.push(state.clone());
                    continue;
                }
                active_main_workers += 1;
                append_runtime_supervision_event(
                    config,
                    Some(state),
                    "RuntimeStalled",
                    &format!(
                        "issue={issue_identifier} stalled_for_ms={} reason={}",
                        stall.stalled_for_ms, stall.reason
                    ),
                )?;
                let reason = format!(
                    "runtime_stalled issue={} stalled_for_ms={}",
                    issue_identifier, stall.stalled_for_ms
                );
                println!("run_loop_resume_preflight action={reason}");
                retained_states.push(state.clone());
                blocked.get_or_insert(reason);
            }
            ResumePreflightAction::Block { reason } => {
                append_runtime_supervision_event(config, Some(state), "ResumeBlocked", &reason)?;
                retained_states.push(state.clone());
                blocked.get_or_insert(reason);
            }
        }
    }

    if recover {
        recover_registered_main_sessions(
            adapter,
            config,
            now_ms,
            &mut retained_states,
            &mut active_main_workers,
            &mut recoverable_states,
        )?;
    }

    Ok(RuntimePreflightSummary {
        retained_states,
        active_main_workers,
        recoverable_states,
        blocked,
    })
}

fn recover_registered_main_sessions(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    config: &RuntimeConfig,
    now_ms: u64,
    retained_states: &mut Vec<RuntimeState>,
    active_main_workers: &mut usize,
    recoverable_states: &mut Vec<RuntimeRecoveryCandidate>,
) -> Result<(), Box<dyn std::error::Error>> {
    let registry = load_session_registry(&session_registry_path(config))?;
    for record in registry.sessions.iter().rev() {
        if !registered_main_runtime_session(record) {
            continue;
        }
        let Some(issue_identifier) = record.issue_identifier.as_deref() else {
            continue;
        };
        if retained_states
            .iter()
            .any(|state| runtime_state_issue_identifier(state) == Some(issue_identifier))
            || recoverable_states.iter().any(|candidate| {
                runtime_state_issue_identifier(&candidate.state) == Some(issue_identifier)
            })
        {
            continue;
        }

        let state = runtime_state_from_session_record(record);
        match runtime_session_probe_from_record(config, record, now_ms) {
            Ok(probe) if session_status_counts_as_active_worker(&probe.status) => {
                if !recover_registry_issue_allows_active_retention(
                    adapter,
                    config,
                    &state,
                    issue_identifier,
                )? {
                    continue;
                }
                *active_main_workers += 1;
                append_runtime_supervision_event(
                    config,
                    Some(&state),
                    "RuntimeRecoverSkippedActiveRegistrySession",
                    &format!(
                        "issue={issue_identifier} session={} status={} source={} evidence={}",
                        record.session_name,
                        probe.status.as_str(),
                        probe.source.as_str(),
                        compact_evidence(&probe.evidence)
                    ),
                )?;
                println!(
                    "run_loop_resume_preflight action=active_registry_session issue={} session={} status={} source={} evidence={}",
                    issue_identifier,
                    record.session_name,
                    probe.status.as_str(),
                    probe.source.as_str(),
                    compact_evidence(&probe.evidence)
                );
                retained_states.push(state);
            }
            Ok(probe) => {
                if !matches!(probe.status, SessionStatus::Stale | SessionStatus::Failed) {
                    continue;
                }
                let Some(issue) =
                    recover_registry_issue_for_restart(adapter, config, &state, issue_identifier)?
                else {
                    continue;
                };
                let reason = format!(
                    "registry_session_recoverable session={} status={} source={} evidence={}",
                    record.session_name,
                    probe.status.as_str(),
                    probe.source.as_str(),
                    compact_evidence(&probe.evidence)
                );
                recover_registered_session_candidate(
                    config,
                    recoverable_states,
                    retained_states,
                    RecoverRegisteredSessionCandidateInput {
                        state: &state,
                        issue,
                        issue_identifier,
                        session_name: &record.session_name,
                        reason: &reason,
                        status: Some((&probe.status, probe.source.as_str())),
                    },
                )?;
            }
            Err(error) => {
                let Some(issue) =
                    recover_registry_issue_for_restart(adapter, config, &state, issue_identifier)?
                else {
                    continue;
                };
                let reason = format!(
                    "registry_session_unavailable session={} reason={}",
                    record.session_name,
                    compact_evidence(&error.to_string())
                );
                recover_registered_session_candidate(
                    config,
                    recoverable_states,
                    retained_states,
                    RecoverRegisteredSessionCandidateInput {
                        state: &state,
                        issue,
                        issue_identifier,
                        session_name: &record.session_name,
                        reason: &reason,
                        status: None,
                    },
                )?;
            }
        }
    }
    Ok(())
}

fn recover_registry_issue_allows_active_retention(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    config: &RuntimeConfig,
    state: &RuntimeState,
    issue_identifier: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    match adapter.get_issue(issue_identifier) {
        Ok(Some(issue)) => Ok(normalize_state(&issue.state) == "in progress"),
        Ok(None) => Ok(false),
        Err(error) => {
            let reason = format!(
                "tracker_read_failed: {}",
                compact_evidence(&error.to_string())
            );
            append_runtime_supervision_event(
                config,
                Some(state),
                "RuntimeRecoverReadSkipped",
                &format!("issue={issue_identifier} reason={reason}"),
            )?;
            println!(
                "run_loop_resume_preflight action=recover_registry_read_skipped issue={} reason={}",
                issue_identifier, reason
            );
            Ok(true)
        }
    }
}

fn recover_registry_issue_for_restart(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    config: &RuntimeConfig,
    state: &RuntimeState,
    issue_identifier: &str,
) -> Result<Option<TrackerIssue>, Box<dyn std::error::Error>> {
    match adapter.get_issue(issue_identifier) {
        Ok(Some(issue)) if normalize_state(&issue.state) == "in progress" => Ok(Some(issue)),
        Ok(_) => Ok(None),
        Err(error) => {
            let reason = format!(
                "tracker_read_failed: {}",
                compact_evidence(&error.to_string())
            );
            append_runtime_supervision_event(
                config,
                Some(state),
                "RuntimeRecoverReadSkipped",
                &format!("issue={issue_identifier} reason={reason}"),
            )?;
            println!(
                "run_loop_resume_preflight action=recover_registry_read_skipped issue={} reason={}",
                issue_identifier, reason
            );
            Ok(None)
        }
    }
}

struct RecoverRegisteredSessionCandidateInput<'a> {
    state: &'a RuntimeState,
    issue: TrackerIssue,
    issue_identifier: &'a str,
    session_name: &'a str,
    reason: &'a str,
    status: Option<(&'a SessionStatus, &'a str)>,
}

fn recover_registered_session_candidate(
    config: &RuntimeConfig,
    recoverable_states: &mut Vec<RuntimeRecoveryCandidate>,
    retained_states: &mut Vec<RuntimeState>,
    input: RecoverRegisteredSessionCandidateInput<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    append_runtime_supervision_event(
        config,
        Some(input.state),
        "RuntimeRecoverable",
        &format!("issue={} reason={}", input.issue_identifier, input.reason),
    )?;
    if let Some((status, source)) = input.status {
        println!(
            "run_loop_resume_preflight action=recoverable_registry_session issue={} session={} status={} source={}",
            input.issue_identifier,
            input.session_name,
            status.as_str(),
            source
        );
    } else {
        println!(
            "run_loop_resume_preflight action=recoverable_registry_session issue={} session={} status=unavailable",
            input.issue_identifier, input.session_name
        );
    }
    recoverable_states.push(RuntimeRecoveryCandidate {
        state: input.state.clone(),
        issue: input.issue,
        reason: input.reason.to_string(),
    });
    retained_states.push(input.state.clone());
    Ok(())
}

fn active_runtime_session_for_issue(
    config: &RuntimeConfig,
    state: &RuntimeState,
    now_ms: u64,
) -> Result<Option<ActiveRuntimeSessionProbe>, Box<dyn std::error::Error>> {
    let Some(issue_identifier) = runtime_state_issue_identifier(state) else {
        return Ok(None);
    };
    let registry = load_session_registry(&session_registry_path(config))?;
    let mut best: Option<(u8, ActiveRuntimeSessionProbe)> = None;
    for record in registry.sessions.iter().rev() {
        if !registered_main_runtime_session(record)
            || record.issue_identifier.as_deref() != Some(issue_identifier)
        {
            continue;
        }
        let Ok(probe) = runtime_session_probe_from_record(config, record, now_ms) else {
            continue;
        };
        if !session_status_counts_as_active_worker(&probe.status) {
            continue;
        }
        let priority = active_session_status_priority(&probe.status);
        if best
            .as_ref()
            .map(|(best_priority, _)| priority < *best_priority)
            .unwrap_or(true)
        {
            best = Some((
                priority,
                ActiveRuntimeSessionProbe {
                    state: runtime_state_from_session_record(record),
                    session_name: record.session_name.clone(),
                    probe,
                },
            ));
        }
    }
    if let Some((_, active_session)) = best {
        return Ok(Some(active_session));
    }

    let Some(probe) = runtime_session_probe_for_state(config, state, now_ms).unwrap_or(None) else {
        return Ok(None);
    };
    if !session_status_counts_as_active_worker(&probe.status) {
        return Ok(None);
    }
    Ok(Some(ActiveRuntimeSessionProbe {
        state: state.clone(),
        session_name: state
            .backend_session_id
            .clone()
            .unwrap_or_else(|| "unknown".into()),
        probe,
    }))
}

fn terminal_runtime_session_for_issue(
    config: &RuntimeConfig,
    state: &RuntimeState,
    now_ms: u64,
) -> Result<Option<ActiveRuntimeSessionProbe>, Box<dyn std::error::Error>> {
    let Some(issue_identifier) = runtime_state_issue_identifier(state) else {
        return Ok(None);
    };
    let registry = load_session_registry(&session_registry_path(config))?;
    for record in registry.sessions.iter().rev() {
        if !registered_main_runtime_session(record)
            || record.issue_identifier.as_deref() != Some(issue_identifier)
        {
            continue;
        }
        let Ok(probe) = runtime_session_probe_from_record(config, record, now_ms) else {
            continue;
        };
        if matches!(probe.status, SessionStatus::Completed) {
            let mut state = runtime_state_from_session_record(record);
            state.last_event = Some("SessionTerminal".into());
            return Ok(Some(ActiveRuntimeSessionProbe {
                state,
                session_name: record.session_name.clone(),
                probe,
            }));
        }
        if session_status_counts_as_active_worker(&probe.status) {
            return Ok(None);
        }
    }

    Ok(None)
}

fn registered_main_runtime_session(record: &AgentSessionRecord) -> bool {
    record.lane.eq_ignore_ascii_case("main") && record.backend != "codex-app-manual"
}

fn runtime_state_issue_identifier(state: &RuntimeState) -> Option<&str> {
    state
        .active_issue
        .as_ref()
        .map(|issue| issue.identifier.as_str())
}

fn runtime_state_from_session_record(record: &AgentSessionRecord) -> RuntimeState {
    let identifier = record
        .issue_identifier
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let mut state = RuntimeState::active(
        RuntimeIssueState {
            id: record
                .issue_id
                .clone()
                .unwrap_or_else(|| identifier.clone()),
            identifier,
        },
        record.backend.clone(),
    );
    state.workspace_path = Some(record.worktree.clone());
    state.branch_name = record.branch.clone();
    state.backend_session_id = Some(record.session_name.clone());
    state.lane = Some(record.lane.clone());
    state.run_id = record.run_id.clone();
    state.backend_log_path = Some(record.log_path.clone());
    state.backend_attach_command = Some(record.attach_command.clone());
    state.profile_id = record.profile_id.clone();
    state.instance_name = record.instance_name.clone();
    state.actor_role = record.actor_role.clone();
    state.actor_label = record.actor_label.clone();
    state.git_author = record.git_author.clone();
    state.attempt_count = record.attempt;
    state.updated_at_ms = Some(record.updated_at_ms);
    state.last_event = Some("SessionRunning".into());
    state
}

fn runtime_session_probe_for_state(
    config: &RuntimeConfig,
    state: &RuntimeState,
    now_ms: u64,
) -> Result<Option<SessionStatusProbe>, Box<dyn std::error::Error>> {
    if !matches!(
        state.last_event.as_deref(),
        Some("SessionRunning" | "SessionTerminal")
    ) {
        return Ok(None);
    }
    let Some(session_id) = state.backend_session_id.as_deref() else {
        return Ok(None);
    };
    let registry = load_session_registry(&session_registry_path(config))?;
    let Some(record) = registry
        .sessions
        .iter()
        .rev()
        .find(|record| record.session_name == session_id)
    else {
        return Ok(None);
    };
    Ok(Some(runtime_session_probe_from_record(
        config, record, now_ms,
    )?))
}

fn runtime_session_probe_from_record(
    config: &RuntimeConfig,
    record: &AgentSessionRecord,
    now_ms: u64,
) -> Result<SessionStatusProbe, Box<dyn std::error::Error>> {
    let is_tmux_session = record.backend == "tmux";
    let pane_tail = if is_tmux_session {
        Some(
            capture_tmux_pane_tail(
                &config.tmux.command,
                &record.pane_target,
                DEFAULT_SESSION_STATUS_LINES,
            )
            .map_err(|error| {
                io::Error::other(format!(
                    "tmux_pane_unavailable session={} error={}",
                    record.session_name,
                    compact_evidence(&error)
                ))
            })?,
        )
    } else {
        None
    };
    let log_tail = if is_tmux_session {
        read_log_tail(&record.log_path, DEFAULT_SESSION_STATUS_LINES)?
    } else {
        None
    };
    Ok(classify_session_record(
        record,
        pane_tail.as_deref(),
        log_tail.as_deref(),
        now_ms,
        DEFAULT_SESSION_STALE_AFTER_MS,
    ))
}

fn session_status_counts_as_active_worker(status: &SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Starting
            | SessionStatus::Running
            | SessionStatus::WaitingForTrust
            | SessionStatus::WaitingForApproval
            | SessionStatus::WaitingForHumanInput
            | SessionStatus::UsageLimited
            | SessionStatus::Unknown
            | SessionStatus::UnknownPersisted(_)
    )
}

fn active_session_status_priority(status: &SessionStatus) -> u8 {
    match status {
        SessionStatus::Running => 0,
        SessionStatus::Starting => 1,
        SessionStatus::WaitingForApproval
        | SessionStatus::WaitingForHumanInput
        | SessionStatus::WaitingForTrust => 2,
        SessionStatus::UsageLimited => 3,
        SessionStatus::Unknown | SessionStatus::UnknownPersisted(_) => 4,
        SessionStatus::Completed
        | SessionStatus::Recorded
        | SessionStatus::Failed
        | SessionStatus::Stale => 5,
    }
}

fn runtime_recovery_reason(
    config: &RuntimeConfig,
    state: &RuntimeState,
    now_ms: u64,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if let Some(retry) = &state.retry {
        if retry.due_in_ms(now_ms) > 0 {
            return Ok(None);
        }
        return Ok(Some(format!(
            "retry_due attempt={} error={}",
            retry.attempt,
            compact_evidence(&retry.error)
        )));
    }

    if let Some(stall) = detect_runtime_stall(state, now_ms, config.codex.stall_timeout_ms) {
        return Ok(Some(format!(
            "runtime_stalled stalled_for_ms={} reason={}",
            stall.stalled_for_ms, stall.reason
        )));
    }

    if !matches!(
        state.last_event.as_deref(),
        Some("SessionRunning" | "SessionTerminal")
    ) {
        return Ok(None);
    }

    let Some(session_id) = state.backend_session_id.as_deref() else {
        return Ok(Some("session_running_without_backend_session_id".into()));
    };

    let registry = load_session_registry(&session_registry_path(config))?;
    let Some(record) = registry
        .sessions
        .iter()
        .rev()
        .find(|record| record.session_name == session_id)
    else {
        return Ok(Some(format!(
            "session_missing_from_registry session={session_id}"
        )));
    };

    match runtime_session_probe_from_record(config, record, now_ms) {
        Ok(probe) if matches!(probe.status, SessionStatus::Stale | SessionStatus::Failed) => {
            return Ok(Some(format!(
                "session_{} source={} evidence={}",
                probe.status.as_str(),
                probe.source.as_str(),
                compact_evidence(&probe.evidence)
            )));
        }
        Ok(_) => {}
        Err(error) => {
            return Ok(Some(compact_evidence(&error.to_string())));
        }
    }

    Ok(None)
}

fn runtime_state_is_main_lane(state: &RuntimeState) -> bool {
    state
        .lane
        .as_deref()
        .map(|lane| lane.eq_ignore_ascii_case("main"))
        .unwrap_or(true)
}

fn runtime_state_points_at_in_progress_issue(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    state: &RuntimeState,
) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(runtime_state_in_progress_issue(adapter, state)?.is_some())
}

fn runtime_state_in_progress_issue(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    state: &RuntimeState,
) -> Result<Option<TrackerIssue>, Box<dyn std::error::Error>> {
    let Some(active_issue) = state.active_issue.as_ref() else {
        return Ok(None);
    };
    let Some(issue) = adapter.get_issue(&active_issue.identifier)? else {
        return Ok(None);
    };
    Ok((normalize_state(&issue.state) == "in progress").then_some(issue))
}

fn stale_runtime_state_action(
    state: &RuntimeState,
    issue: &TrackerIssue,
    normalized_state: &str,
    config: &RuntimeConfig,
) -> Result<ResumePreflightAction, Box<dyn std::error::Error>> {
    let active_issue = state
        .active_issue
        .as_ref()
        .ok_or("runtime state has no active issue")?;
    let archive_reason = if config
        .terminal_state_set()
        .iter()
        .any(|state| state == normalized_state)
    {
        "tracker_state_terminal"
    } else if matches!(normalized_state, "agent review" | "human review") {
        "tracker_state_handoff"
    } else {
        "tracker_state_non_active"
    };

    match runtime_workspace_status(state)? {
        RuntimeWorkspaceStatus::Absent | RuntimeWorkspaceStatus::Clean(_) => {
            Ok(ResumePreflightAction::ArchiveStale {
                issue_identifier: active_issue.identifier.clone(),
                tracker_state: issue.state.clone(),
                archive_reason: archive_reason.into(),
            })
        }
        RuntimeWorkspaceStatus::Dirty(path) => Ok(ResumePreflightAction::Block {
            reason: format!(
                "runtime state references {} but tracker state is {}; workspace is dirty at {}",
                active_issue.identifier,
                issue.state,
                path.display()
            ),
        }),
        RuntimeWorkspaceStatus::Unknown { path, reason } => Ok(ResumePreflightAction::Block {
            reason: format!(
                "runtime state references {} but tracker state is {}; workspace status is unknown at {}: {}",
                active_issue.identifier,
                issue.state,
                path.display(),
                reason
            ),
        }),
    }
}

fn runtime_workspace_status(
    state: &RuntimeState,
) -> Result<RuntimeWorkspaceStatus, Box<dyn std::error::Error>> {
    let Some(path) = state.workspace_path.as_ref() else {
        return Ok(RuntimeWorkspaceStatus::Absent);
    };
    if !path.exists() {
        return Ok(RuntimeWorkspaceStatus::Absent);
    }
    if !path.is_dir() {
        return Ok(RuntimeWorkspaceStatus::Unknown {
            path: path.clone(),
            reason: "workspace path is not a directory".into(),
        });
    }

    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(path)
        .arg("status")
        .arg("--porcelain")
        .output();
    match output {
        Ok(output) if output.status.success() => {
            if output.stdout.is_empty() {
                Ok(RuntimeWorkspaceStatus::Clean(path.clone()))
            } else {
                Ok(RuntimeWorkspaceStatus::Dirty(path.clone()))
            }
        }
        Ok(output) => Ok(RuntimeWorkspaceStatus::Unknown {
            path: path.clone(),
            reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
        Err(error) => Ok(RuntimeWorkspaceStatus::Unknown {
            path: path.clone(),
            reason: error.to_string(),
        }),
    }
}

fn archive_runtime_state(
    config: &RuntimeConfig,
    state: &RuntimeState,
    reason: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let runtime_path = runtime_state_path(config);
    let archive_dir = runtime_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("archive");
    std::fs::create_dir_all(&archive_dir)?;
    let issue_ref = state
        .active_issue
        .as_ref()
        .map(|issue| issue.identifier.as_str())
        .unwrap_or("unknown");
    let archive_path = archive_dir.join(format!(
        "runtime-state-{}-{}-{}.json",
        current_time_ms(),
        sanitize_archive_segment(issue_ref),
        sanitize_archive_segment(reason)
    ));
    std::fs::write(&archive_path, serde_json::to_string_pretty(state)?)?;
    Ok(archive_path)
}

fn sanitize_archive_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    sanitized.trim_matches('-').to_string()
}

fn ensure_write_mode_main_agent_backend(
    workflow_path: &Path,
    config: &RuntimeConfig,
    command: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if config.backend.kind != "dry-run" {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "write-mode {command} is blocked because workflow={} configures main_lane.backend=dry-run; configure a real main-agent backend such as tmux, codex, or claude-code before using --write",
            workflow_path.display()
        ),
    )
    .into())
}

fn no_dispatch_action(limit: Option<usize>, poll_interval_ms: u64) -> NoDispatchAction {
    if limit.is_some() {
        return NoDispatchAction::Stop {
            reason: "no_dispatchable_issue",
        };
    }

    NoDispatchAction::SleepAndContinue {
        delay_ms: poll_interval_ms,
    }
}

fn unbounded_loop_sleep_ms(limit: Option<usize>, poll_interval_ms: u64) -> Option<u64> {
    limit.is_none().then_some(poll_interval_ms)
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn current_gmt_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format_gmt_timestamp(seconds)
}

fn format_gmt_timestamp(seconds_since_unix_epoch: u64) -> String {
    let days = (seconds_since_unix_epoch / 86_400) as i64;
    let seconds_of_day = seconds_since_unix_epoch % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} GMT")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_unix_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

fn lane_claim_for_issue(
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

fn render_prompt_with_claim(
    template: &str,
    issue: &TrackerIssue,
    attempt: Option<u32>,
    claim: Option<&LaneClaim>,
) -> Result<String, jade_symphony::prompt::PromptError> {
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

fn append_runtime_supervision_event(
    config: &RuntimeConfig,
    state: Option<&RuntimeState>,
    event: &str,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    let active_issue = state.and_then(|state| state.active_issue.as_ref());
    log.append(&EventRecord {
        event: event.into(),
        issue_id: active_issue.map(|issue| issue.id.clone()),
        issue_identifier: active_issue.map(|issue| issue.identifier.clone()),
        session_id: state.and_then(|state| state.backend_session_id.clone()),
        profile_id: state.and_then(|state| state.profile_id.clone()),
        instance_name: state.and_then(|state| state.instance_name.clone()),
        actor_role: Some(config.identity.actor_role.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        git_author: config.identity.git.author(),
        tracker_mutation: None,
        message: message.into(),
    })?;
    Ok(())
}

struct TrackerMutationAudit<'a> {
    command: &'a str,
    mutation_type: &'a str,
    issue_ref: Option<&'a str>,
    target: Option<String>,
    from_state: Option<String>,
    to_state: Option<String>,
    reason: &'a str,
}

fn append_tracker_mutation_audit(config: &RuntimeConfig, audit: TrackerMutationAudit<'_>) {
    let issue_ref = audit.issue_ref.map(ToOwned::to_owned);
    let record = TrackerMutationAuditRecord::from_input(TrackerMutationAuditInput {
        command: audit.command.into(),
        mutation_type: audit.mutation_type.into(),
        issue_ref: issue_ref.clone(),
        target: audit.target,
        from_state: audit.from_state,
        to_state: audit.to_state,
        reason: audit.reason.into(),
        timestamp_ms: current_time_ms(),
    });
    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    if let Err(error) = log.append(&EventRecord {
        event: "tracker_mutation".into(),
        issue_id: None,
        issue_identifier: issue_ref,
        session_id: None,
        profile_id: None,
        instance_name: None,
        actor_role: Some(config.identity.actor_role.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        git_author: config.identity.git.author(),
        tracker_mutation: Some(record),
        message: audit.reason.into(),
    }) {
        eprintln!("audit_warning=tracker_mutation_unavailable reason={error}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackerMutationOutcome {
    Applied,
    AlreadyApplied,
    Recovered,
}

impl TrackerMutationOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::AlreadyApplied => "already_applied",
            Self::Recovered => "recovered",
        }
    }

    fn should_record_audit(self) -> bool {
        !matches!(self, Self::AlreadyApplied)
    }
}

fn tracker_recovery_marker(key: &str) -> String {
    format!(
        "<!-- jade-symphony-tracker-recovery key={} -->",
        recovery_key_component(key)
    )
}

fn ensure_tracker_recovery_marker(markdown: &str, key: &str) -> String {
    let marker = tracker_recovery_marker(key);
    if markdown.contains(&marker) {
        return markdown.to_string();
    }

    let mut body = markdown.trim_end().to_string();
    if !body.is_empty() {
        body.push_str("\n\n");
    }
    body.push_str(&marker);
    body
}

fn recovery_key(label: &str, issue_ref: &str, seed: &str) -> String {
    format!(
        "{}-{}-{}",
        recovery_key_component(label),
        recovery_key_component(issue_ref),
        stable_recovery_hash(seed)
    )
}

fn recovery_key_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

fn stable_recovery_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn issue_has_recovery_marker(issue: &TrackerIssue, key: &str) -> bool {
    issue
        .description
        .as_deref()
        .is_some_and(|description| description.contains(&tracker_recovery_marker(key)))
}

fn issue_project_field_matches(issue: &TrackerIssue, field_name: &str, value: &str) -> bool {
    project_text_field(issue, field_name).as_deref() == Some(value)
}

fn issue_state_matches(issue: &TrackerIssue, normalized_state: &str) -> bool {
    tracker_state_match_key(&issue.state) == tracker_state_match_key(normalized_state)
}

fn issue_is_closed(issue: &TrackerIssue) -> bool {
    issue
        .project_fields
        .get("GitHub Issue State")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|state| tracker_state_match_key(state) == "closed")
}

fn tracker_state_match_key(value: &str) -> String {
    normalize_state(value).replace('_', " ")
}

fn recoverable_project_state_failure(kind: ProjectStateFailureKind) -> bool {
    matches!(
        kind,
        ProjectStateFailureKind::Network
            | ProjectStateFailureKind::TransientBackend
            | ProjectStateFailureKind::RateLimit
    )
}

fn tracker_recovery_next_action(kind: ProjectStateFailureKind) -> &'static str {
    match kind {
        ProjectStateFailureKind::RateLimit => "wait_then_readback_or_rerun_same_lane",
        ProjectStateFailureKind::TransientBackend => "wait_then_readback_or_rerun_same_lane",
        ProjectStateFailureKind::Network => "readback_or_rerun_same_lane",
        _ => "human_input_required",
    }
}

fn recover_after_tracker_error<F>(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    mutation_type: &str,
    error: TrackerError,
    expected: F,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>>
where
    F: Fn(&TrackerIssue) -> bool,
{
    let kind = classify_project_state_error(&error);
    if !recoverable_project_state_failure(kind) {
        return Err(error.into());
    }

    match adapter.get_issue(issue_ref) {
        Ok(Some(issue)) if expected(&issue) => {
            println!(
                "tracker_recovery action=recovered mutation_type={} issue={} failure_kind={} next=continue",
                mutation_type,
                issue_ref,
                kind.as_str()
            );
            Ok(TrackerMutationOutcome::Recovered)
        }
        Ok(Some(issue)) => Err(format!(
            "recoverable_tracker_mutation_uncertain mutation_type={} issue={} failure_kind={} current_state={:?} next={} error={}",
            mutation_type,
            issue_ref,
            kind.as_str(),
            issue.state,
            tracker_recovery_next_action(kind),
            error
        )
        .into()),
        Ok(None) => Err(format!(
            "recoverable_tracker_mutation_uncertain mutation_type={} issue={} failure_kind={} current_state=missing next={} error={}",
            mutation_type,
            issue_ref,
            kind.as_str(),
            tracker_recovery_next_action(kind),
            error
        )
        .into()),
        Err(readback_error) => {
            let readback_kind = classify_project_state_error(&readback_error);
            Err(format!(
                "recoverable_tracker_mutation_readback_failed mutation_type={} issue={} failure_kind={} readback_kind={} next={} error={} readback_error={}",
                mutation_type,
                issue_ref,
                kind.as_str(),
                readback_kind.as_str(),
                tracker_recovery_next_action(kind),
                error,
                readback_error
            )
            .into())
        }
    }
}

fn set_project_field_with_recovery(
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    assignment: &ProjectFieldAssignment,
    mutation_type: &str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    if issue_project_field_matches(issue, &assignment.name, &assignment.value) {
        println!(
            "tracker_recovery action=already_applied mutation_type={} issue={} field={:?}",
            mutation_type, issue.identifier, assignment.name
        );
        return Ok(TrackerMutationOutcome::AlreadyApplied);
    }

    match adapter.set_project_field(&issue.identifier, assignment) {
        Ok(()) => Ok(TrackerMutationOutcome::Applied),
        Err(error) => recover_after_tracker_error(
            adapter,
            &issue.identifier,
            mutation_type,
            error,
            |readback| issue_project_field_matches(readback, &assignment.name, &assignment.value),
        ),
    }
}

fn set_state_with_recovery(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
    normalized_state: &str,
    mutation_type: &str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    if initial_issue.is_some_and(|issue| issue_state_matches(issue, normalized_state)) {
        println!(
            "tracker_recovery action=already_applied mutation_type={} issue={} state={}",
            mutation_type, issue_ref, normalized_state
        );
        return Ok(TrackerMutationOutcome::AlreadyApplied);
    }

    match adapter.set_state(issue_ref, normalized_state) {
        Ok(()) => Ok(TrackerMutationOutcome::Applied),
        Err(error) => {
            recover_after_tracker_error(adapter, issue_ref, mutation_type, error, |readback| {
                issue_state_matches(readback, normalized_state)
            })
        }
    }
}

fn upsert_workpad_with_recovery(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
    markdown: &str,
    key: &str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    if initial_issue.is_some_and(|issue| issue_has_recovery_marker(issue, key)) {
        println!(
            "tracker_recovery action=already_applied mutation_type=workpad_write issue={} key={}",
            issue_ref,
            recovery_key_component(key)
        );
        return Ok(TrackerMutationOutcome::AlreadyApplied);
    }

    let body = ensure_tracker_recovery_marker(markdown, key);
    match adapter.upsert_workpad(issue_ref, &body) {
        Ok(()) => Ok(TrackerMutationOutcome::Applied),
        Err(error) => {
            recover_after_tracker_error(adapter, issue_ref, "workpad_write", error, |readback| {
                issue_has_recovery_marker(readback, key)
            })
        }
    }
}

fn add_timeline_comment_with_recovery(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
    markdown: &str,
    key: &str,
    mutation_type: &str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    if initial_issue.is_some_and(|issue| issue_has_recovery_marker(issue, key)) {
        println!(
            "tracker_recovery action=already_applied mutation_type={} issue={} key={}",
            mutation_type,
            issue_ref,
            recovery_key_component(key)
        );
        return Ok(TrackerMutationOutcome::AlreadyApplied);
    }

    let body = ensure_tracker_recovery_marker(markdown, key);
    match adapter.add_issue_comment(issue_ref, &body) {
        Ok(()) => Ok(TrackerMutationOutcome::Applied),
        Err(error) => {
            recover_after_tracker_error(adapter, issue_ref, mutation_type, error, |readback| {
                issue_has_recovery_marker(readback, key)
            })
        }
    }
}

fn close_issue_with_recovery(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    if initial_issue.is_some_and(issue_is_closed) {
        println!(
            "tracker_recovery action=already_applied mutation_type=issue_close issue={issue_ref}"
        );
        return Ok(TrackerMutationOutcome::AlreadyApplied);
    }

    match adapter.close_issue(issue_ref) {
        Ok(()) => Ok(TrackerMutationOutcome::Applied),
        Err(TrackerError::NotImplemented(message)) => {
            eprintln!("merge_once_warning=issue_close_unavailable reason={message}");
            Ok(TrackerMutationOutcome::AlreadyApplied)
        }
        Err(error) => {
            recover_after_tracker_error(adapter, issue_ref, "issue_close", error, issue_is_closed)
        }
    }
}

fn merge_completion_recovery_key(issue: &TrackerIssue, pr_ref: &str) -> String {
    let run = project_text_field(issue, "Merging Agent")
        .as_deref()
        .and_then(|value| LaneClaim::parse(value).ok())
        .map(|claim| claim.run)
        .unwrap_or_else(|| "run-not-recorded".into());
    recovery_key(
        "merge-completion",
        &issue.identifier,
        &format!("{}|{}|{}", issue.identifier, run, pr_ref),
    )
}

fn merge_decision_recovery_key(
    issue: &TrackerIssue,
    decision: &jade_symphony::merge_lane::MergeLaneDecision,
) -> String {
    recovery_key(
        "merge-decision",
        &issue.identifier,
        &format!(
            "{}|{:?}|{}|{}",
            issue.identifier,
            decision.kind,
            decision.pr_url.as_deref().unwrap_or("missing"),
            decision.target_state.unwrap_or("none")
        ),
    )
}

fn merge_pull_request_with_recovery(
    pr_ref: &str,
    runner: &dyn HandoffCommandRunner,
    cwd: &Path,
) -> Result<(CommandOutput, TrackerMutationOutcome), Box<dyn std::error::Error>> {
    match merge_pull_request(pr_ref, runner, cwd) {
        Ok(output) => Ok((output, TrackerMutationOutcome::Applied)),
        Err(error) => {
            let kind = classify_project_state_failure_message(&error.to_string());
            if !recoverable_project_state_failure(kind) {
                return Err(error.into());
            }
            match fetch_pull_request_status_with_recheck(pr_ref, runner, cwd, 2) {
                Ok(status) if normalize_state(&status.state) == "merged" => {
                    println!(
                        "tracker_recovery action=recovered mutation_type=pr_merge pr={} failure_kind={} next=continue",
                        pr_ref,
                        kind.as_str()
                    );
                    Ok((
                        CommandOutput {
                            status: 0,
                            stdout: format!(
                                "merge command failed after possible server-side success; readback shows PR merged: {pr_ref}"
                            ),
                            stderr: error.to_string(),
                        },
                        TrackerMutationOutcome::Recovered,
                    ))
                }
                Ok(status) => Err(format!(
                    "recoverable_tracker_mutation_uncertain mutation_type=pr_merge pr={} failure_kind={} readback_state={} next={} error={}",
                    pr_ref,
                    kind.as_str(),
                    status.state,
                    tracker_recovery_next_action(kind),
                    error
                )
                .into()),
                Err(readback_error) => Err(format!(
                    "recoverable_tracker_mutation_readback_failed mutation_type=pr_merge pr={} failure_kind={} next={} error={} readback_error={}",
                    pr_ref,
                    kind.as_str(),
                    tracker_recovery_next_action(kind),
                    error,
                    readback_error
                )
                .into()),
            }
        }
    }
}

fn run_loop_runtime_state_for_issue(
    existing: Option<&RuntimeState>,
    issue: &TrackerIssue,
    config: &RuntimeConfig,
    event: &str,
    claim: &LaneClaim,
) -> RuntimeState {
    if event == "Resumed" {
        if let Some(existing) = existing
            .filter(|state| {
                runtime_state_issue_identifier(state) == Some(issue.identifier.as_str())
            })
            .filter(|state| state.last_event.as_deref() == Some("SessionTerminal"))
        {
            let mut state = existing.clone();
            state.run_id.get_or_insert_with(|| claim.run.clone());
            return state;
        }
    }

    let profile = selected_execution_profile(&config.profiles).ok().flatten();
    let mut state = RuntimeState::active(
        RuntimeIssueState {
            id: issue.id.clone(),
            identifier: issue.identifier.clone(),
        },
        &config.backend.kind,
    );
    state.attempt_count = next_runtime_attempt_count(existing, &issue.identifier);
    state.branch_name = issue.branch_name.clone();
    state.lane = Some("main".into());
    state.run_id = Some(claim.run.clone());
    state.profile_id = profile.as_ref().map(|profile| profile.profile_id.clone());
    state.instance_name = profile
        .as_ref()
        .map(|profile| profile.instance_name.clone());
    state.actor_role = Some(config.identity.actor_role.clone());
    state.actor_label = Some(config.identity.actor_label.clone());
    state.git_author = config.identity.git.author();
    state.last_event = Some(event.into());
    state
}

fn next_runtime_attempt_count(existing: Option<&RuntimeState>, issue_identifier: &str) -> u32 {
    existing
        .and_then(|state| {
            state
                .active_issue
                .as_ref()
                .filter(|issue| issue.identifier == issue_identifier)
                .map(|_| state.attempt_count.saturating_add(1))
        })
        .unwrap_or(1)
}

fn run_loop_runtime_state_with_result(
    mut state: RuntimeState,
    result: &IssueExecutionResult,
) -> RuntimeState {
    state.workspace_path = Some(result.workspace_path.clone());
    state.backend = result.backend.clone();
    state.backend_session_id = result.session_id.clone();
    state.run_id = result.run_id.clone();
    state.backend_log_path = result.backend_log_path.clone();
    state.backend_attach_command = result.backend_attach_command.clone();
    state.profile_id = result.profile_id.clone();
    state.instance_name = result.instance_name.clone();
    state.actor_role = Some(result.actor_role.clone());
    state.actor_label = Some(result.actor_label.clone());
    state.git_author = result.git_author.clone();
    state.last_event = Some(if result.pending_session {
        "SessionRunning".into()
    } else if result.success {
        "Completed".into()
    } else {
        "Failed".into()
    });
    state
}

fn run_loop_runtime_state_with_transition(
    mut state: RuntimeState,
    from: Option<String>,
    to: &str,
    reason: &str,
) -> RuntimeState {
    state.last_transition = Some(RuntimeTransition {
        from,
        to: to.into(),
        reason: reason.into(),
    });
    state
}

fn reconcile_pending_main_session(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    handoff: &IssueHandoffPlan,
    state: &RuntimeState,
) -> Result<Option<MainSessionReconciliation>, Box<dyn std::error::Error>> {
    let Some(active_issue) = state.active_issue.as_ref() else {
        return Ok(None);
    };
    if active_issue.identifier != issue.identifier {
        return Ok(None);
    }
    if !matches!(
        state.last_event.as_deref(),
        Some("SessionRunning" | "SessionTerminal")
    ) {
        return Ok(None);
    }

    let Some(session_id) = state.backend_session_id.as_deref() else {
        return Ok(Some(MainSessionReconciliation::Active {
            status: "unknown".into(),
            source: "runtime".into(),
            evidence: "runtime state records SessionRunning without backend session id".into(),
        }));
    };

    let registry = load_session_registry(&session_registry_path(config))?;
    let Some(record) = registry
        .sessions
        .iter()
        .rev()
        .find(|record| record.session_name == session_id)
    else {
        return Ok(Some(MainSessionReconciliation::Active {
            status: "unknown".into(),
            source: "runtime".into(),
            evidence: format!("runtime session {session_id} is missing from session registry"),
        }));
    };

    let is_tmux_session = record.backend == "tmux";
    let pane_tail = if is_tmux_session {
        match capture_tmux_pane_tail(
            &config.tmux.command,
            &record.pane_target,
            DEFAULT_SESSION_STATUS_LINES,
        ) {
            Ok(tail) => Some(tail),
            Err(error) => {
                return Ok(Some(MainSessionReconciliation::Active {
                    status: "unknown".into(),
                    source: "tmux".into(),
                    evidence: format!(
                        "tmux pane unavailable for session {session_id}: {}",
                        compact_evidence(&error)
                    ),
                }))
            }
        }
    } else {
        None
    };
    let log_tail = if is_tmux_session {
        read_log_tail(&record.log_path, DEFAULT_SESSION_STATUS_LINES)?
    } else {
        None
    };
    let probe = classify_session_record(
        record,
        pane_tail.as_deref(),
        log_tail.as_deref(),
        unix_timestamp_ms(),
        DEFAULT_SESSION_STALE_AFTER_MS,
    );

    match probe.status {
        SessionStatus::Completed => Ok(Some(MainSessionReconciliation::Terminal(Box::new(
            result_from_reconciled_main_session(
                config,
                handoff,
                state,
                record,
                true,
                None,
                probe.evidence,
            ),
        )))),
        SessionStatus::Failed => Ok(Some(MainSessionReconciliation::Terminal(Box::new(
            result_from_reconciled_main_session(
                config,
                handoff,
                state,
                record,
                false,
                None,
                format!("main session failed: {}", probe.evidence),
            ),
        )))),
        SessionStatus::UsageLimited => Ok(Some(MainSessionReconciliation::Terminal(Box::new(
            result_from_reconciled_main_session(
                config,
                handoff,
                state,
                record,
                false,
                Some(UsageLimitPause {
                    classifier: "usage_limit".into(),
                    evidence: probe.evidence.clone(),
                }),
                format!("main session usage-limited: {}", probe.evidence),
            ),
        )))),
        _ => Ok(Some(MainSessionReconciliation::Active {
            status: probe.status.as_str().into(),
            source: probe.source.as_str().into(),
            evidence: probe.evidence,
        })),
    }
}

fn result_from_reconciled_main_session(
    config: &RuntimeConfig,
    handoff: &IssueHandoffPlan,
    state: &RuntimeState,
    record: &AgentSessionRecord,
    success: bool,
    usage_limit_pause: Option<UsageLimitPause>,
    message: String,
) -> IssueExecutionResult {
    IssueExecutionResult {
        workspace_path: state
            .workspace_path
            .clone()
            .unwrap_or_else(|| handoff.workspace_path.clone()),
        backend: state.backend.clone(),
        profile_id: state.profile_id.clone(),
        instance_name: state.instance_name.clone(),
        success,
        pending_session: false,
        session_id: state
            .backend_session_id
            .clone()
            .or_else(|| Some(record.session_name.clone())),
        run_id: state.run_id.clone().or_else(|| record.run_id.clone()),
        backend_log_path: state
            .backend_log_path
            .clone()
            .or_else(|| Some(record.log_path.clone())),
        backend_attach_command: state
            .backend_attach_command
            .clone()
            .or_else(|| Some(record.attach_command.clone())),
        message,
        usage_limit_pause,
        prompt_artifact_path: Some(record.prompt_artifact_path.clone()),
        actor_role: state
            .actor_role
            .clone()
            .unwrap_or_else(|| config.identity.actor_role.clone()),
        actor_label: state
            .actor_label
            .clone()
            .unwrap_or_else(|| config.identity.actor_label.clone()),
        git_author: state
            .git_author
            .clone()
            .or_else(|| config.identity.git.author()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::NotConfigured,
            author: state.git_author.clone(),
            applied_keys: Vec::new(),
        },
        live_handoff: None,
        handoff_verification: None,
    }
}

fn run_loop_handoff_plan(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<IssueHandoffPlan, HandoffError> {
    let profile = selected_execution_profile(&config.profiles)
        .ok()
        .flatten()
        .map(|profile| profile.workspace_namespace);
    let mut plan = plan_issue_handoff_for_profile(
        &config.workspace.root,
        issue,
        DEFAULT_RUN_LOOP_BASE_BRANCH,
        profile.as_deref(),
    )?;

    if issue.normalized_state() == "rework" {
        if let Ok(repo_root) = std::env::current_dir() {
            if let Ok(report) = discover_issue_workspaces(config, issue, &repo_root) {
                if let Some(candidate) = report
                    .canonical_index
                    .and_then(|index| report.candidates.get(index))
                {
                    if candidate.branch.as_deref() == Some(plan.branch_name.as_str()) {
                        plan.workspace_path = candidate.path.clone();
                    }
                }
            }
        }
    }

    Ok(plan)
}

fn run_loop_apply_recovery_handoff(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    handoff: &mut IssueHandoffPlan,
    state: &RuntimeState,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if !matches!(
        state.last_event.as_deref(),
        Some("SessionRunning" | "SessionTerminal")
    ) {
        return Ok(None);
    }

    if let Some(path) = state.workspace_path.as_ref() {
        if let Some(branch) = current_git_branch(path)? {
            apply_recovery_worktree_to_handoff(config, issue, handoff, path, &branch)?;
            return Ok(Some(format!(
                "source=runtime_state workspace={} branch={}",
                path.display(),
                branch
            )));
        }
    }

    let repo_root = std::env::current_dir()?;
    let report = discover_issue_workspaces(config, issue, &repo_root)?;
    if let Some(candidate) = report
        .canonical_index
        .and_then(|index| report.candidates.get(index))
    {
        let Some(branch) = candidate.branch.as_deref() else {
            return Ok(None);
        };
        apply_recovery_worktree_to_handoff(config, issue, handoff, &candidate.path, branch)?;
        return Ok(Some(format!(
            "source=workspace_discovery workspace={} branch={}",
            candidate.path.display(),
            branch
        )));
    }

    Ok(None)
}

fn apply_recovery_worktree_to_handoff(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    handoff: &mut IssueHandoffPlan,
    path: &Path,
    branch: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let inferred_issue = infer_issue_ref_from_branch_or_path(Some(branch), path);
    if inferred_issue.as_deref() != Some(issue.identifier.as_str()) && branch != handoff.branch_name
    {
        return Err(format!(
            "recover refuses worktree {} on branch {}; it does not match issue {}",
            path.display(),
            branch,
            issue.identifier
        )
        .into());
    }

    handoff.workspace_key = recovery_workspace_key(config, path)?;
    handoff.workspace_path = path.to_path_buf();
    handoff.branch_name = branch.to_string();
    handoff.pull_request.head_branch = branch.to_string();
    Ok(())
}

fn recovery_workspace_key(
    config: &RuntimeConfig,
    path: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let root = canonicalize_or_self(&config.workspace.root);
    let path = canonicalize_or_self(path);
    if !path.starts_with(&root) {
        return Err(format!(
            "recover refuses worktree outside configured workspace root: {} not under {}",
            path.display(),
            root.display()
        )
        .into());
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(format!(
            "recover cannot derive workspace key from path {}",
            path.display()
        )
        .into());
    };
    Ok(name.to_string())
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn run_loop_runtime_ownership(
    issue: &TrackerIssue,
    config: &RuntimeConfig,
    handoff: &IssueHandoffPlan,
) -> Result<RuntimeOwnershipMarker, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    Ok(RuntimeOwnershipMarker {
        issue_ref: issue.identifier.clone(),
        actor_role: config.identity.actor_role.clone(),
        actor_label: config.identity.actor_label.clone(),
        profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
        instance_name: profile
            .as_ref()
            .map(|profile| profile.instance_name.clone()),
        workspace_key: handoff.workspace_key.clone(),
        branch_name: handoff.branch_name.clone(),
    })
}

fn run_loop_ownership_workpad(
    issue: &TrackerIssue,
    ownership: &RuntimeOwnershipMarker,
    event: &str,
    claim: &LaneClaim,
) -> String {
    [
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Runtime Ownership".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Event: `{event}`"),
        format!("- Run: `{}`", claim.run),
        format!("- Claim: `{}`", claim.render()),
        "- This marker is advisory tracker-visible ownership for active `In Progress` work.".into(),
        "- Another main loop profile should not resume this issue when the marker differs.".into(),
        String::new(),
        render_runtime_ownership_marker(ownership),
    ]
    .join("\n")
}

fn run_loop_live_handoff_enabled(config: &RuntimeConfig) -> bool {
    config.tracker.kind == "github_project_v2" && config.tracker.fixture_path.is_none()
}

fn run_handoff_verification(workspace_path: &Path, config: &RuntimeConfig) -> HandoffVerification {
    if config.verification.commands.is_empty() {
        return HandoffVerification {
            success: true,
            summary: "skipped:not_configured".into(),
        };
    }

    for (index, command) in config.verification.commands.iter().enumerate() {
        let label = format!("verification:{}", index + 1);
        if let Err(error) = run_workspace_command(
            &label,
            command,
            workspace_path,
            config.verification.timeout_ms,
        ) {
            return HandoffVerification {
                success: false,
                summary: format!(
                    "failed command={} index={} error={}",
                    shell_summary(command),
                    index + 1,
                    compact_evidence(&error.to_string())
                ),
            };
        }
    }

    HandoffVerification {
        success: true,
        summary: format!("passed:{} command(s)", config.verification.commands.len()),
    }
}

fn shell_summary(command: &str) -> String {
    let compact = compact_evidence(command);
    format!("`{compact}`")
}

fn compact_evidence(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 240;
    let truncated = compact.chars().take(LIMIT).collect::<String>();
    if truncated.len() < compact.len() {
        format!("{truncated}...")
    } else {
        compact
    }
}

fn handle_run_loop_gate_failure(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    issue: &TrackerIssue,
    decision: &GateDecision,
    options: &RunLoopOptions,
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    print_latest_status(&latest_status_for_issue(
        config,
        issue,
        "main",
        "blocked",
        "quality_gate_failed",
        Some(gate_target_state(decision).into()),
    ));
    println!(
        "run_loop_gate=failed issue={} decision={:?}",
        issue.identifier, decision.kind
    );
    if options.write {
        adapter.upsert_workpad(&issue.identifier, &gate_workpad(issue, decision))?;
        adapter.set_state(&issue.identifier, gate_target_state(decision))?;
    } else {
        println!(
            "run_loop_dry_run action=workpad issue={} reason=quality_gate_failed",
            issue.identifier
        );
        println!(
            "run_loop_dry_run action=set_state issue={} target_state={}",
            issue.identifier,
            gate_target_state(decision)
        );
    }
    Ok(())
}

fn handle_run_loop_handoff_failure(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    issue: &TrackerIssue,
    error: &HandoffError,
    options: &RunLoopOptions,
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    print_latest_status(&latest_status_for_issue(
        config,
        issue,
        "main",
        "blocked",
        "handoff_plan_failed",
        Some("Need Human Input".into()),
    ));
    println!(
        "run_loop_handoff=failed issue={} error={}",
        issue.identifier, error
    );
    let workpad = run_loop_handoff_failure_workpad(issue, error);
    if options.write {
        adapter.upsert_workpad(&issue.identifier, &workpad)?;
        adapter.set_state(&issue.identifier, "need_human_input")?;
    } else {
        println!(
            "run_loop_dry_run action=workpad issue={} reason=handoff_plan_failed",
            issue.identifier
        );
        println!(
            "run_loop_dry_run action=set_state issue={} target_state=need_human_input",
            issue.identifier
        );
    }
    Ok(())
}

fn print_run_loop_dry_run_actions(
    issue: &TrackerIssue,
    handoff: &IssueHandoffPlan,
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    let smoke_gate = main_app_server_smoke_gate(config);
    if normalize_state(&issue.state) != "in progress" {
        println!(
            "run_loop_dry_run action=claim issue={} target_state=in_progress",
            issue.identifier
        );
    } else {
        println!("run_loop_dry_run action=resume issue={}", issue.identifier);
    }
    println!(
        "run_loop_dry_run action=handoff_plan issue={} workspace_key={} workspace_path={} branch={} pr_title={:?}",
        issue.identifier,
        handoff.workspace_key,
        handoff.workspace_path.display(),
        handoff.branch_name,
        handoff.pull_request.title
    );
    println!(
        "run_loop_dry_run action=identity issue={} actor_role={} actor_label={:?} git_author={:?}",
        issue.identifier,
        config.identity.actor_role,
        config.identity.actor_label,
        config.identity.git.author()
    );
    println!(
        "run_loop_dry_run action=run issue={} backend={} backend_source={} command={} approval_policy={} app_server_live_smoke_ready={} session_registry={}",
        issue.identifier,
        smoke_gate.backend,
        smoke_gate.backend_source,
        shell_quote_display(&smoke_gate.command),
        smoke_gate.approval_policy,
        smoke_gate.app_server_live_smoke_ready,
        session_registry_path(config).display()
    );
    if let Some(profile) = profile {
        println!(
            "run_loop_dry_run profile_id={} instance_name={}",
            profile.profile_id, profile.instance_name
        );
    }
    println!(
        "run_loop_dry_run action=worktree issue={} workspace={} branch={}",
        issue.identifier,
        handoff.workspace_path.display(),
        handoff.branch_name
    );
    let verification_summary = if config.verification.commands.is_empty() {
        "skipped:not_configured".to_string()
    } else {
        format!(
            "configured:{} command(s)",
            config.verification.commands.len()
        )
    };
    println!(
        "run_loop_dry_run action=verify issue={} summary={}",
        issue.identifier, verification_summary
    );
    println!(
        "run_loop_dry_run action=pr issue={} head={} base={}",
        issue.identifier, handoff.branch_name, handoff.pull_request.base_branch
    );
    println!(
        "run_loop_dry_run action=pr_ready issue={} mode=if_draft command=\"gh pr ready <linked-pr>\"",
        issue.identifier
    );
    println!(
        "run_loop_dry_run action=workpad issue={} evidence=run_summary",
        issue.identifier
    );
    println!(
        "run_loop_dry_run action=handoff issue={} target_state=agent_review",
        issue.identifier
    );
    Ok(())
}

fn run_loop_handoff_workpad(
    issue: &TrackerIssue,
    result: &IssueExecutionResult,
    handoff: &IssueHandoffPlan,
    ownership: Option<&RuntimeOwnershipMarker>,
) -> String {
    let mut lines = vec![
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Context".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Source: `jade-symphony main loop`".to_string(),
        String::new(),
        "### Plan".to_string(),
        "- [x] Read the issue contract, Project state, Main Workpad, and timeline evidence."
            .to_string(),
        "- [x] Prepare or resume the isolated issue workspace and branch.".to_string(),
        "- [x] Run the configured Main Agent backend for the implementation slice.".to_string(),
        "- [x] Verify handoff evidence and prepare the PR for Agent Review.".to_string(),
        String::new(),
        "### Work Log".to_string(),
        format!(
            "- Run `{}` executed with backend `{}`.",
            result.run_id.as_deref().unwrap_or("n/a"),
            result.backend
        ),
        format!(
            "- Workspace `{}` was used for implementation evidence.",
            result.workspace_path.display()
        ),
        format!("- Backend message: {}", result.message),
        String::new(),
        "### Run Evidence".to_string(),
        format!("- Run: `{}`", result.run_id.as_deref().unwrap_or("n/a")),
        format!("- Workspace: `{}`", result.workspace_path.display()),
        format!("- Backend: `{}`", result.backend),
        format!(
            "- Profile: `{}`",
            result.profile_id.as_deref().unwrap_or("n/a")
        ),
        format!(
            "- Instance: `{}`",
            result.instance_name.as_deref().unwrap_or("n/a")
        ),
        format!("- Actor role: `{}`", result.actor_role),
        format!("- Actor label: `{}`", result.actor_label),
        format!(
            "- Git author: `{}`",
            result.git_author.as_deref().unwrap_or("n/a")
        ),
        format!("- Git identity: `{}`", result.git_identity.summary()),
        format!("- Success: `{}`", result.success),
        format!(
            "- Session: `{}`",
            result.session_id.as_deref().unwrap_or("n/a")
        ),
        format!(
            "- Session status: `{}`",
            if result.pending_session {
                "running"
            } else {
                "terminal"
            }
        ),
        format!(
            "- Attach command: `{}`",
            result.backend_attach_command.as_deref().unwrap_or("n/a")
        ),
        format!(
            "- Session log: `{}`",
            result
                .backend_log_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "n/a".into())
        ),
        format!("- Message: {}", result.message),
        String::new(),
        "### Planned Handoff".to_string(),
        format!("- Workspace key: `{}`", handoff.workspace_key),
        format!("- Workspace path: `{}`", handoff.workspace_path.display()),
        format!("- Branch: `{}`", handoff.branch_name),
        format!("- PR title: `{}`", handoff.pull_request.title),
        format!("- PR base branch: `{}`", handoff.pull_request.base_branch),
        format!("- Branch target role: `{:?}`", handoff.branch_target.role),
        branch_target_workpad_line(handoff),
        rework_continuation_workpad_line(handoff),
        handoff_verification_workpad_line(result),
        live_handoff_workpad_line(result),
        String::new(),
        "### Main-Agent Boundary".to_string(),
        "- Locally complete main-agent work stops at `Agent Review`.".to_string(),
        "- `Human Review` is reserved for independent Review Agent pass evidence.".to_string(),
    ];

    if let Some(ownership) = ownership {
        lines.push(String::new());
        lines.push(render_runtime_ownership_marker(ownership));
    }

    lines.join("\n")
}

fn branch_target_workpad_line(handoff: &IssueHandoffPlan) -> String {
    let mut parts = Vec::new();
    if let Some(parent_issue) = &handoff.branch_target.parent_issue {
        parts.push(format!("native_parent={parent_issue}"));
    }
    if let Some(parent_integration_branch) = &handoff.branch_target.parent_integration_branch {
        parts.push(format!(
            "parent_integration_branch={parent_integration_branch}"
        ));
    }
    if let Some(parent_final_base_branch) = &handoff.branch_target.parent_final_base_branch {
        parts.push(format!("parent_final_base={parent_final_base_branch}"));
    }

    if parts.is_empty() {
        "- Branch target evidence: `single-issue default`".to_string()
    } else {
        format!("- Branch target evidence: `{}`", parts.join(" "))
    }
}

fn rework_continuation_workpad_line(handoff: &IssueHandoffPlan) -> String {
    match &handoff.continuation {
        Some(continuation) => format!(
            "- Rework continuation: `{}` from `{}` ({}) branch=`{}`",
            continuation.pull_request_url,
            continuation.source,
            continuation.pull_request_state,
            continuation.branch_name.as_deref().unwrap_or("unknown")
        ),
        None => "- Rework continuation: `not-used`".to_string(),
    }
}

fn handoff_verification_workpad_line(result: &IssueExecutionResult) -> String {
    format!(
        "- Handoff verification: `{}`",
        result
            .handoff_verification
            .as_deref()
            .unwrap_or("skipped:not_run")
    )
}

fn live_handoff_workpad_line(result: &IssueExecutionResult) -> String {
    match &result.live_handoff {
        Some(handoff) => {
            let ready = handoff
                .pull_request_ready
                .as_ref()
                .map(|status| {
                    format!(
                        "ready-check: `was_draft={} marked_ready={}`",
                        status.was_draft, status.marked_ready
                    )
                })
                .unwrap_or_else(|| "ready-check: `not-run`".into());
            format!(
                "- Live PR: `{}` (created: `{}`, branch pushed: `{}`, verification: `{}`, {})",
                handoff.publication.pr_url,
                handoff.publication.pr_created,
                handoff.publication.branch_pushed,
                handoff.verification,
                ready
            )
        }
        None => "- Live PR: `not-created`".to_string(),
    }
}

fn record_live_handoff_pr_link(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    result: &IssueExecutionResult,
) -> Result<(), String> {
    let Some(handoff) = &result.live_handoff else {
        return Ok(());
    };

    let linked = adapter
        .list_linked_pull_requests(issue_ref)
        .map_err(|error| format!("handoff PR link verification failed: {error}"))?;
    if linked_pull_requests_contain(&linked, &handoff.publication.pr_url) {
        return Ok(());
    }

    adapter
        .link_pull_request(issue_ref, &handoff.publication.pr_url)
        .map_err(|error| format!("handoff PR link repair failed: {error}"))?;

    let linked = adapter
        .list_linked_pull_requests(issue_ref)
        .map_err(|error| format!("handoff PR link verification failed: {error}"))?;

    if linked_pull_requests_contain(&linked, &handoff.publication.pr_url) {
        Ok(())
    } else {
        Err(format!(
            "handoff PR link was not Project-visible after repair attempt: {}",
            handoff.publication.pr_url
        ))
    }
}

fn apply_live_handoff_pr_link(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    result: &mut IssueExecutionResult,
) -> bool {
    if result.live_handoff.is_none() {
        return false;
    }

    match record_live_handoff_pr_link(adapter, issue_ref, result) {
        Ok(()) => {
            if let Some(handoff) = result.live_handoff.as_mut() {
                handoff.project_pr_link_verified = Some(true);
            }
            true
        }
        Err(error) => {
            if let Some(handoff) = result.live_handoff.as_mut() {
                handoff.project_pr_link_verified = Some(false);
            }
            result.success = false;
            result.message = error;
            false
        }
    }
}

fn linked_pull_requests_contain(
    linked_pull_requests: &[jade_symphony::model::LinkedPullRequest],
    pr_url: &str,
) -> bool {
    let expected_url = pr_url.trim();
    let expected_number = pull_request_number_from_ref(expected_url);
    linked_pull_requests.iter().any(|linked| {
        linked
            .url
            .as_deref()
            .is_some_and(|url| url.trim() == expected_url)
            || expected_number.is_some() && linked.number == expected_number
    })
}

fn pull_request_number_from_ref(reference: &str) -> Option<u64> {
    pull_request_number_from_url(reference).or_else(|| {
        reference
            .trim()
            .trim_start_matches('#')
            .trim_start_matches("PR_")
            .parse()
            .ok()
    })
}

fn pull_request_number_from_url(url: &str) -> Option<u64> {
    url.trim()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .and_then(|segment| segment.parse().ok())
}

fn run_loop_agent_review_handoff_evidence(
    issue: &TrackerIssue,
    result: &IssueExecutionResult,
    handoff: &IssueHandoffPlan,
    main_workpad_markdown: Option<&str>,
) -> AgentReviewHandoffEvidence {
    let mut evidence = AgentReviewHandoffEvidence::from_plan(
        handoff,
        format!(
            "backend={} success={} session={} message={}",
            result.backend,
            result.success,
            result.session_id.as_deref().unwrap_or("n/a"),
            result.message
        ),
        "main agent completed local run",
    );
    evidence.record_main_workpad_markdown(main_workpad_markdown);
    evidence.pull_request_url = result
        .live_handoff
        .as_ref()
        .map(|handoff| handoff.publication.pr_url.clone())
        .or_else(|| {
            issue
                .linked_pull_requests
                .iter()
                .find_map(|pr| pr.url.clone())
        });
    evidence.pull_request_is_draft = result
        .live_handoff
        .as_ref()
        .and_then(|handoff| {
            handoff
                .pull_request_ready
                .as_ref()
                .map(|ready| ready.was_draft && !ready.marked_ready)
        })
        .or_else(|| {
            let url = evidence.pull_request_url.as_deref()?;
            issue
                .linked_pull_requests
                .iter()
                .find(|pr| pr.url.as_deref() == Some(url))
                .and_then(|pr| pr.is_draft)
        });
    evidence.project_pr_link_verified = result
        .live_handoff
        .as_ref()
        .and_then(|handoff| handoff.project_pr_link_verified)
        .or_else(|| {
            let url = evidence.pull_request_url.as_deref()?;
            Some(linked_pull_requests_contain(
                &issue.linked_pull_requests,
                url,
            ))
        });
    if evidence.pull_request_url.is_none() {
        evidence.no_pr_blocker = Some(
            "No pull request URL was present in tracker data at handoff time; keeping issue out of Agent Review until PR evidence is durable.".into(),
        );
    }
    evidence
}

fn run_loop_handoff_failure_workpad(issue: &TrackerIssue, error: &HandoffError) -> String {
    [
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Context".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Source: `jade-symphony main loop`".to_string(),
        String::new(),
        "### Handoff Planning Blocker".to_string(),
        format!("- Error: `{}`", error),
        "- Backend execution was skipped before claim/run to avoid mixing issue scope.".to_string(),
        String::new(),
        "### Required Human Decision".to_string(),
        "- Confirm the correct branch/workspace ownership before retrying.".to_string(),
    ]
    .join("\n")
}

fn run_loop_assignee_ownership_workpad(issue: &TrackerIssue, reason: &str) -> String {
    [
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Assignee Ownership Blocker".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Reason: {reason}"),
        format!("- Issue assignees: `{}`", issue.assignees.join(", ")),
        String::new(),
        "### Boundary".to_string(),
        "- Jade Symphony did not claim this issue or move it to `In Progress`.".to_string(),
        "- Assign the issue to the active GitHub identity or selected execution profile before retrying.".to_string(),
    ]
    .join("\n")
}

fn run_loop_usage_limit_pause_workpad(
    issue: &TrackerIssue,
    result: &IssueExecutionResult,
    pause: &UsageLimitPause,
    retry_delay_ms: u64,
) -> String {
    [
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Usage-Limit Pause".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Source: `jade-symphony main loop`".to_string(),
        format!("- Backend: `{}`", result.backend),
        format!("- Classifier: `{}`", pause.classifier),
        format!("- Evidence: {}", pause.evidence),
        format!("- Retry backoff: `{retry_delay_ms}ms`"),
        String::new(),
        "### State Safety".to_string(),
        "- Tracker state was not advanced to `Agent Review`.".to_string(),
        "- Runtime state keeps the active issue and next retry time.".to_string(),
        "- The main loop will skip this issue until retry backoff expires or an operator intervenes."
            .to_string(),
    ]
    .join("\n")
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Plan {
        workflow_path: PathBuf,
        json: bool,
    },
    AutopilotPlan {
        workflow_path: PathBuf,
        json: bool,
    },
    StatusApi {
        workflow_path: PathBuf,
        bind: SocketAddr,
        once: bool,
    },
    Validate {
        workflow_path: PathBuf,
    },
    Inspect {
        workflow_path: PathBuf,
        states: Vec<String>,
    },
    ProjectState {
        options: ProjectStateOptions,
    },
    ProjectIssue {
        workflow_path: PathBuf,
        issue_ref: String,
        json: bool,
    },
    ProjectInspect {
        workflow_path: PathBuf,
        issue_ref: String,
        lane: Option<AgentSessionLaneArg>,
    },
    Doctor {
        options: DoctorOptions,
    },
    DoctorRepairHumanReview {
        workflow_path: PathBuf,
        write: bool,
    },
    SkillsStatus {
        input: SkillStatusInput,
        json: bool,
    },
    Profiles {
        workflow_path: PathBuf,
    },
    Debug {
        workflow_path: PathBuf,
    },
    CleanupPlan {
        workflow_path: PathBuf,
    },
    CleanPlan {
        workflow_path: PathBuf,
    },
    CleanAudit {
        workflow_path: PathBuf,
    },
    RunOnce {
        workflow_path: PathBuf,
    },
    RunLoop {
        options: RunLoopOptions,
    },
    CleanupWorkspaces {
        workflow_path: PathBuf,
        write: bool,
    },
    WorkspaceList {
        workflow_path: PathBuf,
    },
    WorkspaceShow {
        workflow_path: PathBuf,
        issue_ref: String,
    },
    WorkspaceAdopt {
        workflow_path: PathBuf,
        issue_ref: String,
        path: PathBuf,
        write: bool,
    },
    WorkspaceEnsure {
        workflow_path: PathBuf,
        issue_ref: String,
        pr_ref: Option<String>,
        branch: Option<String>,
        write: bool,
    },
    MergeOnce {
        workflow_path: PathBuf,
        write: bool,
    },
    SetState {
        workflow_path: PathBuf,
        issue_ref: String,
        state: String,
        write: bool,
    },
    Workpad {
        workflow_path: PathBuf,
        issue_ref: String,
        markdown_path: PathBuf,
        write: bool,
    },
    TimelineComment {
        workflow_path: PathBuf,
        issue_ref: String,
        markdown_path: PathBuf,
        write: bool,
    },
    LinkPr {
        workflow_path: PathBuf,
        issue_ref: String,
        pr_ref: String,
        write: bool,
    },
    CreateFollowUp {
        workflow_path: PathBuf,
        title: String,
        body_path: PathBuf,
        write: bool,
    },
    AddToProject {
        workflow_path: PathBuf,
        issue_id: String,
        write: bool,
    },
    ReviewFake {
        workflow_path: PathBuf,
        issue_ref: String,
        outcome: FakeReviewOutcome,
        write: bool,
    },
    ReviewOnce {
        workflow_path: PathBuf,
        issue_ref: String,
        write: bool,
    },
    ReviewClaim {
        workflow_path: PathBuf,
        issue_ref: String,
        worker: String,
        write: bool,
    },
    LaneClaim {
        workflow_path: PathBuf,
        issue_ref: String,
        lane: AgentSessionLaneArg,
        worker: String,
        source: CliLaneClaimSource,
        write: bool,
    },
    ReviewClearClaim {
        workflow_path: PathBuf,
        issue_ref: String,
        write: bool,
    },
    ReviewPass {
        workflow_path: PathBuf,
        issue_ref: String,
        evidence: String,
        write: bool,
    },
    ReviewReject {
        workflow_path: PathBuf,
        issue_ref: String,
        evidence: String,
        target_state: String,
        write: bool,
    },
    ReviewSession {
        workflow_path: PathBuf,
        issue_ref: String,
        write: bool,
    },
    ReviewFreshness {
        input: ReviewFreshnessInput,
    },
    ReviewLoop {
        options: ReviewLoopOptions,
    },
    ReviewStatus {
        options: ReviewStatusCliOptions,
    },
    MergeSession {
        workflow_path: PathBuf,
        issue_ref: String,
        write: bool,
    },
    AgentSessionStart {
        workflow_path: PathBuf,
        issue_ref: String,
        lane: AgentSessionLaneArg,
        run_id: Option<String>,
        write: bool,
    },
    SessionStart {
        workflow_path: PathBuf,
        issue_ref: String,
        lane: AgentSessionLaneArg,
        run_id: String,
        write: bool,
    },
    SessionList {
        workflow_path: PathBuf,
    },
    SessionAttach {
        workflow_path: PathBuf,
        session: String,
        exec: bool,
    },
    AgentSessionList {
        workflow_path: PathBuf,
    },
    AgentSessionAttach {
        workflow_path: PathBuf,
        session: String,
        exec: bool,
    },
    MergeLoop {
        options: MergeLoopOptions,
    },
    Gate {
        workflow_path: PathBuf,
        issue_ref: String,
        apply: bool,
        write: bool,
    },
    ForgeValidate {
        workflow_path: PathBuf,
        status: Option<ForgeStatusArg>,
        title: String,
        markdown: String,
        issue_ref: Option<String>,
    },
    ForgeCreate {
        workflow_path: PathBuf,
        title: String,
        markdown: String,
        status: ForgeStatusArg,
        project: Option<String>,
        project_fields: Vec<ProjectFieldAssignment>,
        assignees: Vec<String>,
        write: bool,
        dry_run: bool,
    },
    ForgePromote {
        workflow_path: PathBuf,
        issue_ref: String,
        title: String,
        markdown: String,
        promotion_note: PromotionNoteInput,
        write: bool,
        dry_run: bool,
    },
    ForgeRework {
        options: ForgeReworkOptions,
    },
    Help(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunLoopOptions {
    workflow_path: PathBuf,
    max_iterations: Option<usize>,
    once: bool,
    write: bool,
    recover: bool,
    max_concurrent: Option<usize>,
    display: DisplayMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectStateOptions {
    workflow_path: PathBuf,
    display: DisplayMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewLoopOptions {
    workflow_path: PathBuf,
    max_iterations: Option<usize>,
    once: bool,
    write: bool,
    fake_outcome: Option<FakeReviewOutcome>,
    max_concurrent: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewStatusCliOptions {
    workflow_path: PathBuf,
    issue_filter: Option<String>,
    recent_limit: usize,
    verbose: bool,
    json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorOptions {
    workflow_path: Option<PathBuf>,
    json: bool,
    strict: bool,
    display: DisplayMode,
    interactive: bool,
    auto_fix: bool,
    write: bool,
    stale_after_ms: u64,
    action: Option<DoctorAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DoctorAction {
    Repair(DoctorRepairIssueOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorRepairIssueOptions {
    issue_ref: String,
    write: bool,
    move_need_human_input: bool,
    mark_pr_ready: bool,
    confirm_handoff_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MergeLoopOptions {
    workflow_path: PathBuf,
    max_iterations: Option<usize>,
    once: bool,
    write: bool,
    recover: bool,
    max_concurrent: Option<usize>,
}

impl ReviewLoopOptions {
    fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else {
            self.max_iterations
        }
    }

    fn worker_limit(&self, config: &RuntimeConfig) -> usize {
        self.max_concurrent
            .unwrap_or(config.review.max_concurrent_workers)
            .max(1)
    }
}

impl MergeLoopOptions {
    fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else {
            self.max_iterations
        }
    }

    fn worker_limit(&self, config: &RuntimeConfig) -> usize {
        self.max_concurrent
            .unwrap_or(config.merge_lane.max_concurrent_workers)
            .max(1)
    }
}

impl RunLoopOptions {
    fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else {
            self.max_iterations
        }
    }

    fn worker_limit(&self, config: &RuntimeConfig) -> usize {
        self.max_concurrent
            .unwrap_or(config.agent.max_concurrent_agents)
            .max(1)
    }
}

impl Command {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        if matches!(
            args.first().map(String::as_str),
            Some("help" | "--help" | "-h")
        ) {
            return Ok(Self::Help(usage()));
        }

        let argv = std::iter::once("jade-symphony".to_string())
            .chain(args)
            .collect::<Vec<_>>();
        match Cli::try_parse_from(argv) {
            Ok(cli) => Command::try_from(cli),
            Err(error) if error.kind() == ErrorKind::DisplayHelp => {
                Ok(Self::Help(error.to_string()))
            }
            Err(error) => Err(error.to_string()),
        }
    }
}

fn lane_command(lane: AgentSessionLaneArg, args: LaneCommandArgs) -> Result<Command, String> {
    match args.command {
        MainCommandArgs::Claim(claim) => Ok(Command::LaneClaim {
            workflow_path: claim.workflow_path,
            issue_ref: claim.issue_ref,
            lane,
            worker: claim.worker,
            source: claim.source,
            write: claim.write,
        }),
        MainCommandArgs::Once(args) if lane == AgentSessionLaneArg::Main => Ok(Command::RunOnce {
            workflow_path: args.workflow_path,
        }),
        MainCommandArgs::Loop(args) if lane == AgentSessionLaneArg::Main => run_loop_command(args),
        MainCommandArgs::Once(_) | MainCommandArgs::Loop(_) => {
            Err("only the main lane supports once/loop through this command group".into())
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "jade-symphony",
    about = "OpenAI Symphony-style orchestration harness with Jade Symphony extensions",
    disable_help_subcommand = true,
    arg_required_else_help = false
)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    #[command(
        next_help_heading = "Human / Operator operations",
        alias = "plan-dispatch",
        alias = "dry-run"
    )]
    Plan(WorkflowPathArgs),
    #[command(alias = "validate-workflow")]
    Validate(WorkflowPathArgs),
    #[command(alias = "audit-project")]
    Doctor(DoctorArgs),
    #[command(name = "doctor-repair-human-review")]
    DoctorRepairHumanReview(DoctorRepairArgs),
    #[command(next_help_heading = "Human / Operator operations")]
    Skills(SkillsArgs),
    Profiles(WorkflowPathArgs),
    Debug(WorkflowPathArgs),
    #[command(
        next_help_heading = "Lane orchestration",
        name = "autopilot",
        about = "Read-only all-lane planning preflight"
    )]
    Autopilot(AutopilotArgs),
    Status(StatusArgs),
    Clean(CleanArgs),
    #[command(
        next_help_heading = "Project / Agent internals",
        about = "Discover and record per-issue git worktrees",
        long_about = "Discover and record per-issue git worktrees.\n\n`workspace` is the safe local-worktree coordination surface for Main, Review, and Merge lanes. It discovers existing issue worktrees from the session registry, Main Workpad/timeline evidence, linked PR/branch hints, and `git worktree list`. It can ensure missing Review/Merge inspection worktrees under the configured workspace root, but it never runs `gh pr checkout`, switches branches, or changes the canonical repository checkout.\n\nUse `workspace show` before local Review or Merge inspection. Use `workspace adopt` only when an operator has selected an existing worktree that should become the canonical workspace evidence for the issue. Use `workspace ensure` only when no suitable candidate exists and local inspection is required."
    )]
    Workspace(WorkspaceArgs),
    #[command(name = "session")]
    Session(SessionArgs),
    Project(ProjectArgs),
    #[command(next_help_heading = "Lane orchestration", name = "main")]
    Main(LaneCommandArgs),
    #[command(name = "merge")]
    Merge(MergeArgs),
    Review(ReviewArgs),
    #[command(name = "create-follow-up")]
    CreateFollowUp(CreateFollowUpArgs),
    #[command(next_help_heading = "Issue Forge")]
    Forge(ForgeArgs),
    #[command(
        next_help_heading = "Reserved lifecycle topology",
        about = "Reserved for future all-lane automatic orchestration"
    )]
    Run,
    #[command(about = "Reserved for future Jade Symphony binary and skill upgrades")]
    Upgrade,
}

#[derive(Debug, Args)]
struct WorkflowPathArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    json: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct AutopilotArgs {
    #[command(subcommand)]
    command: AutopilotCommandArgs,
}

#[derive(Debug, Subcommand)]
enum AutopilotCommandArgs {
    #[command(
        about = "Plan Main, Review, and Merge lanes without mutating tracker or runtime state"
    )]
    Plan(AutopilotPlanArgs),
}

#[derive(Debug, Args)]
struct AutopilotPlanArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ProjectStateArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long, value_enum, default_value_t = CliDisplayMode::Plain)]
    display: CliDisplayMode,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct ProjectIssueArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    json: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct InspectArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long = "state")]
    states: Vec<String>,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct DoctorRepairArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: Option<PathBuf>,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    strict: bool,
    #[arg(long, value_enum, default_value_t = CliDisplayMode::Plain)]
    display: CliDisplayMode,
    #[arg(long)]
    interactive: bool,
    #[arg(long = "auto-fix")]
    auto_fix: bool,
    #[arg(long = "stale-after-ms", default_value_t = 10_800_000)]
    stale_after_ms: u64,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    write: bool,
    #[command(subcommand)]
    action: Option<DoctorSubcommandArgs>,
}

#[derive(Debug, Args)]
struct SkillsArgs {
    #[command(subcommand)]
    command: SkillsCommandArgs,
}

#[derive(Debug, Subcommand)]
enum SkillsCommandArgs {
    #[command(about = "Report per-repo Jade Symphony skill readiness")]
    Status(SkillsStatusArgs),
}

#[derive(Debug, Args)]
struct SkillsStatusArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long = "suite-path")]
    suite_path: Option<PathBuf>,
    #[arg(long = "codex-dir")]
    codex_dir: Option<PathBuf>,
    #[arg(long = "gemini-dir")]
    gemini_dir: Option<PathBuf>,
    #[arg(long = "require-gemini")]
    require_gemini: bool,
    #[arg(long = "session-skills")]
    session_skills: Vec<String>,
    #[arg(long = "session-skills-file")]
    session_skills_file: Option<PathBuf>,
    #[arg(long)]
    json: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[command(subcommand)]
    command: StatusCommandArgs,
}

#[derive(Debug, Subcommand)]
enum StatusCommandArgs {
    #[command(about = "Render the current runtime snapshot")]
    Show(WorkflowPathArgs),
    #[command(about = "Serve the current runtime snapshot once over loopback HTTP")]
    Serve(StatusApiArgs),
}

#[derive(Debug, Subcommand)]
enum DoctorSubcommandArgs {
    Repair(DoctorRepairIssueArgs),
}

#[derive(Debug, Args)]
struct DoctorRepairIssueArgs {
    issue_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "move-need-human-input")]
    move_need_human_input: bool,
    #[arg(long = "mark-pr-ready")]
    mark_pr_ready: bool,
    #[arg(long = "confirm-handoff-ready")]
    confirm_handoff_ready: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct StatusApiArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: SocketAddr,
    #[arg(long)]
    once: bool,
}

#[derive(Debug, Args)]
struct RunLoopArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    max_iterations: Option<usize>,
    #[arg(long)]
    once: bool,
    #[arg(long)]
    write: bool,
    #[arg(
        long,
        conflicts_with = "no_recover",
        help = "Enable recover-first handling for interrupted Main runtime sessions (default in --write mode)"
    )]
    recover: bool,
    #[arg(
        long = "no-recover",
        conflicts_with = "recover",
        help = "Disable default recover-first handling in --write mode"
    )]
    no_recover: bool,
    #[arg(long = "max-concurrent")]
    max_concurrent: Option<usize>,
    #[arg(long, value_enum, default_value_t = CliDisplayMode::Plain)]
    display: CliDisplayMode,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    Plain,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliDisplayMode {
    Plain,
    Tui,
}

impl From<CliDisplayMode> for DisplayMode {
    fn from(value: CliDisplayMode) -> Self {
        match value {
            CliDisplayMode::Plain => Self::Plain,
            CliDisplayMode::Tui => Self::Tui,
        }
    }
}

#[derive(Debug, Args)]
struct CleanArgs {
    #[command(subcommand)]
    command: CleanCommand,
}

#[derive(Debug, Subcommand)]
enum CleanCommand {
    Plan(WorkflowPathArgs),
    Audit(WorkflowPathArgs),
}

#[derive(Debug, Args)]
struct CleanupWorkspacesArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct WorkspaceArgs {
    #[command(subcommand)]
    command: WorkspaceCommandArgs,
}

#[derive(Debug, Subcommand)]
enum WorkspaceCommandArgs {
    #[command(
        about = "List discovered issue worktrees and orphan hints",
        long_about = "List discovered issue worktrees and orphan hints.\n\nThis is a read-only Project-wide inventory. It scans tracker issues, session registry records, Main Workpad/timeline evidence, linked PR/branch hints, and local `git worktree list` output. It reports candidates per issue and orphan-looking worktrees whose branch/path implies an issue not currently present in the fetched Project state."
    )]
    List(WorkspaceListArgs),
    #[command(
        about = "Show candidate worktrees for one issue",
        long_about = "Show candidate worktrees for one issue.\n\nThis is the read-only preflight for Review and Merge agents before touching local files. It prints candidate worktrees, their branch/head metadata, evidence sources, warnings, and the canonical candidate when one can be chosen safely. Multiple strong candidates require operator choice through `workspace adopt` before local inspection should rely on a path."
    )]
    Show(WorkspaceShowArgs),
    #[command(
        about = "Record an operator-selected existing worktree",
        long_about = "Record an operator-selected existing worktree as canonical workspace evidence for one issue.\n\n`workspace adopt` validates that the path is an existing git worktree for this repository and that its branch matches the issue/PR evidence. With `--write`, it writes a tracker workpad entry so later Main, Review, and Merge sessions can reuse the same workspace. It does not create a worktree, checkout a PR, switch branches, or mutate files in the selected worktree."
    )]
    Adopt(WorkspaceAdoptArgs),
    #[command(
        about = "Ensure a safe Review/Merge inspection worktree",
        long_about = "Ensure a safe Review/Merge inspection worktree for one issue.\n\n`workspace ensure` first runs the same discovery as `workspace show` and reuses one suitable existing issue worktree when it can be chosen safely. If no suitable worktree exists, it prepares a git worktree only under the workflow-configured workspace root, using the linked PR branch or an explicit `--pr` / `--branch` argument. It never runs `gh pr checkout`, never switches the canonical checkout, refuses ambiguous candidates, and with `--write` records durable Workspace Evidence in the canonical issue workpad."
    )]
    Ensure(WorkspaceEnsureArgs),
}

#[derive(Debug, Args)]
struct WorkspaceListArgs {
    #[arg(
        value_name = "path-to-WORKFLOW.md",
        help = "Workflow config that defines the tracker, artifact roots, and workspace root"
    )]
    workflow_path: PathBuf,
}

#[derive(Debug, Args)]
struct WorkspaceShowArgs {
    #[arg(
        value_name = "path-to-WORKFLOW.md",
        help = "Workflow config that defines the tracker, artifact roots, and workspace root"
    )]
    workflow_path: PathBuf,
    #[arg(help = "Issue identifier to inspect, for example #253")]
    issue_ref: String,
}

#[derive(Debug, Args)]
struct WorkspaceAdoptArgs {
    #[arg(
        value_name = "path-to-WORKFLOW.md",
        help = "Workflow config that defines the tracker, artifact roots, and workspace root"
    )]
    workflow_path: PathBuf,
    #[arg(help = "Issue identifier whose canonical workspace evidence should be updated")]
    issue_ref: String,
    #[arg(help = "Existing local git worktree path selected by the operator")]
    path: PathBuf,
    #[arg(
        long,
        help = "Write workspace adoption evidence to the tracker workpad"
    )]
    write: bool,
    #[arg(
        long = "dry-run",
        help = "Preview adoption validation without writing tracker evidence"
    )]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct WorkspaceEnsureArgs {
    #[arg(
        value_name = "path-to-WORKFLOW.md",
        help = "Workflow config that defines the tracker, artifact roots, and workspace root"
    )]
    workflow_path: PathBuf,
    #[arg(help = "Issue identifier whose Review/Merge inspection workspace should be ensured")]
    issue_ref: String,
    #[arg(
        long = "pr",
        help = "Optional PR number, URL, or ref to fetch when tracker-linked PR evidence is missing or ambiguous"
    )]
    pr_ref: Option<String>,
    #[arg(
        long,
        help = "Optional branch/ref to use instead of the linked PR head branch"
    )]
    branch: Option<String>,
    #[arg(
        long,
        help = "Create or reuse the worktree and write Workspace Evidence"
    )]
    write: bool,
    #[arg(
        long = "dry-run",
        help = "Preview the reuse/create plan without creating worktrees or writing tracker evidence"
    )]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct MergeOnceArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct MergeLoopArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    max_iterations: Option<usize>,
    #[arg(long)]
    once: bool,
    #[arg(long)]
    write: bool,
    #[arg(
        long,
        conflicts_with = "no_recover",
        help = "Enable recover-first handling for interrupted Merge loop claims (default in --write mode)"
    )]
    recover: bool,
    #[arg(
        long = "no-recover",
        conflicts_with = "recover",
        help = "Disable default recover-first handling in --write mode"
    )]
    no_recover: bool,
    #[arg(long = "max-concurrent")]
    max_concurrent: Option<usize>,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct LaneCommandArgs {
    #[command(subcommand)]
    command: MainCommandArgs,
}

#[derive(Debug, Subcommand)]
enum MainCommandArgs {
    Claim(LaneClaimArgs),
    Once(WorkflowPathArgs),
    Loop(RunLoopArgs),
}

#[derive(Debug, Args)]
struct LaneClaimArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    worker: String,
    #[arg(long, value_enum, default_value_t = CliLaneClaimSource::Manual)]
    source: CliLaneClaimSource,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliLaneClaimSource {
    Manual,
    Loop,
    Goal,
}

impl From<CliLaneClaimSource> for LaneClaimSource {
    fn from(value: CliLaneClaimSource) -> Self {
        match value {
            CliLaneClaimSource::Manual => Self::Manual,
            CliLaneClaimSource::Loop => Self::Loop,
            CliLaneClaimSource::Goal => Self::Goal,
        }
    }
}

#[derive(Debug, Args)]
struct AgentSessionArgs {
    #[command(subcommand)]
    command: AgentSessionCommand,
}

#[derive(Debug, Subcommand)]
enum AgentSessionCommand {
    Start(AgentSessionStartArgs),
    List(AgentSessionListArgs),
    Attach(AgentSessionAttachArgs),
}

#[derive(Debug, Args)]
struct AgentSessionStartArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long, value_enum, default_value = "main")]
    lane: AgentSessionLaneArg,
    #[arg(long = "run")]
    run_id: Option<String>,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct SessionArgs {
    #[command(subcommand)]
    command: SessionCommand,
}

#[derive(Debug, Args)]
struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommandArgs,
}

#[derive(Debug, Subcommand)]
enum ProjectCommandArgs {
    #[command(about = "Read tracker state and Project health")]
    State(ProjectStateArgs),
    #[command(about = "Read one Project issue and linked PR evidence")]
    Issue(ProjectIssueArgs),
    #[command(about = "Inspect live issue readiness without mutating tracker state")]
    Inspect(ProjectInspectArgs),
    #[command(name = "set-state", about = "Set one issue Project status")]
    SetState(SetStateArgs),
    #[command(name = "link-pr", about = "Record pull request evidence for one issue")]
    LinkPr(LinkPrArgs),
    #[command(name = "add", about = "Add one GitHub issue to the configured Project")]
    Add(AddToProjectArgs),
    #[command(about = "Upsert the canonical issue workpad")]
    Workpad(WorkpadArgs),
    #[command(
        name = "timeline-comment",
        about = "Append a standalone issue timeline comment"
    )]
    TimelineComment(TimelineCommentArgs),
}

#[derive(Debug, Args)]
struct ProjectInspectArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(help = "Issue identifier to inspect, for example #284")]
    issue_ref: String,
    #[arg(long, value_enum, help = "Optional lane context for readiness output")]
    lane: Option<AgentSessionLaneArg>,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct TimelineCommentArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(value_name = "MARKDOWN_PATH")]
    markdown_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct MergeArgs {
    #[command(subcommand)]
    command: MergeCommandArgs,
}

#[derive(Debug, Subcommand)]
enum MergeCommandArgs {
    Claim(LaneClaimArgs),
    Once(MergeOnceArgs),
    Loop(MergeLoopArgs),
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Start(SessionStartArgs),
    List(AgentSessionListArgs),
    Attach(AgentSessionAttachArgs),
}

#[derive(Debug, Args)]
struct SessionStartArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long, value_enum)]
    lane: AgentSessionLaneArg,
    #[arg(long = "run")]
    run_id: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct AgentSessionListArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
}

#[derive(Debug, Args)]
struct AgentSessionAttachArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    session: String,
    #[arg(long)]
    exec: bool,
}

#[derive(Debug, Args)]
struct LaneSessionAliasArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct GateArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct SetStateArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    state: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct WorkpadArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    markdown_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct LinkPrArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    pr_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct CreateFollowUpArgs {
    #[arg(long)]
    workflow: PathBuf,
    #[arg(long)]
    title: String,
    #[arg(long = "body-file")]
    body_file: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct AddToProjectArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_id: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewFakeArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long, value_enum, default_value = "pass")]
    outcome: CliFakeReviewOutcome,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewOnceArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewClaimArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    worker: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewClearClaimArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewEvidenceArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long = "evidence-file")]
    evidence_file: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewRejectArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long = "evidence-file")]
    evidence_file: PathBuf,
    #[arg(long = "target-state", default_value = "agent_review")]
    target_state: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewFreshnessArgs {
    #[arg(long = "issue")]
    issue_ref: String,
    #[arg(long = "prior-head")]
    prior_head_sha: String,
    #[arg(long = "current-head")]
    current_head_sha: String,
    #[arg(long = "prior-base")]
    prior_base_sha: String,
    #[arg(long = "current-base")]
    current_base_sha: String,
    #[arg(long = "changed-file")]
    changed_files: Vec<String>,
    #[arg(long = "stale-reason", value_enum)]
    stale_reason: CliReviewStaleReason,
    #[arg(long = "rework-class", value_enum)]
    rework_class: CliReviewReworkClass,
    #[arg(long = "patch-summary")]
    patch_summary: Option<String>,
}

#[derive(Debug, Args)]
struct ReviewLoopArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    max_iterations: Option<usize>,
    #[arg(long)]
    once: bool,
    #[arg(long)]
    write: bool,
    #[arg(long = "max-concurrent")]
    max_concurrent: Option<usize>,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "fake-outcome", value_enum)]
    fake_outcome: Option<CliFakeReviewOutcome>,
}

#[derive(Debug, Args)]
struct ReviewStatusArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long = "issue", help = "Filter status to one issue, for example #313")]
    issue_filter: Option<String>,
    #[arg(
        long = "recent",
        default_value_t = DEFAULT_RECENT_REVIEW_JOBS,
        help = "Number of recent completed or failed review jobs to show"
    )]
    recent_limit: usize,
    #[arg(long, help = "Show more paths and anomaly details")]
    verbose: bool,
    #[arg(long, help = "Print the complete structured review status payload")]
    json: bool,
}

#[derive(Debug, Args)]
struct ReviewArgs {
    #[command(subcommand)]
    command: ReviewCommandArgs,
}

#[derive(Debug, Subcommand)]
enum ReviewCommandArgs {
    Fake(ReviewFakeArgs),
    Once(ReviewOnceArgs),
    Claim(LaneClaimArgs),
    Pass(ReviewEvidenceArgs),
    Reject(ReviewRejectArgs),
    Session(LaneSessionAliasArgs),
    Freshness(ReviewFreshnessArgs),
    Loop(ReviewLoopArgs),
    Status(ReviewStatusArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliReviewStaleReason {
    MergeConflict,
    BaseBranchUpdated,
    ReviewOutdated,
    Unknown,
}

impl From<CliReviewStaleReason> for ReviewStaleReason {
    fn from(value: CliReviewStaleReason) -> Self {
        match value {
            CliReviewStaleReason::MergeConflict => ReviewStaleReason::MergeConflict,
            CliReviewStaleReason::BaseBranchUpdated => ReviewStaleReason::BaseBranchUpdated,
            CliReviewStaleReason::ReviewOutdated => ReviewStaleReason::ReviewOutdated,
            CliReviewStaleReason::Unknown => ReviewStaleReason::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliReviewReworkClass {
    MechanicalConflictResolution,
    BaseRefresh,
    SemanticChange,
    Unknown,
}

impl From<CliReviewReworkClass> for ReviewReworkClass {
    fn from(value: CliReviewReworkClass) -> Self {
        match value {
            CliReviewReworkClass::MechanicalConflictResolution => {
                ReviewReworkClass::MechanicalConflictResolution
            }
            CliReviewReworkClass::BaseRefresh => ReviewReworkClass::BaseRefresh,
            CliReviewReworkClass::SemanticChange => ReviewReworkClass::SemanticChange,
            CliReviewReworkClass::Unknown => ReviewReworkClass::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliFakeReviewOutcome {
    Pass,
    Confirmed,
    Failed,
}

impl From<CliFakeReviewOutcome> for FakeReviewOutcome {
    fn from(value: CliFakeReviewOutcome) -> Self {
        match value {
            CliFakeReviewOutcome::Pass => FakeReviewOutcome::Pass,
            CliFakeReviewOutcome::Confirmed => FakeReviewOutcome::ConfirmedFinding,
            CliFakeReviewOutcome::Failed => FakeReviewOutcome::Failed,
        }
    }
}

#[derive(Debug, Args)]
struct ForgeMarkdownArgs {
    #[arg(long)]
    body: Option<String>,
    #[arg(long = "body-file", alias = "file")]
    body_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ForgeArgs {
    #[command(subcommand)]
    command: ForgeCommandArgs,
}

#[derive(Debug, Subcommand)]
enum ForgeCommandArgs {
    Create(ForgeCreateArgs),
    Promote(ForgePromoteArgs),
    Rework(ForgeReworkArgs),
    Validate(ForgeValidateArgs),
}

#[derive(Debug, Args)]
struct ForgeCreateArgs {
    #[arg(long, default_value = "workflows/jade-symphony.md")]
    workflow: PathBuf,
    #[arg(long)]
    title: String,
    #[command(flatten)]
    markdown: ForgeMarkdownArgs,
    #[arg(long, value_enum, ignore_case = true, default_value_t = ForgeStatusArg::Todo)]
    status: ForgeStatusArg,
    #[arg(long)]
    project: Option<String>,
    #[arg(long = "project-field")]
    project_fields: Vec<String>,
    #[arg(long = "assignee")]
    assignees: Vec<String>,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ForgePromoteArgs {
    issue_ref: String,
    #[arg(long, default_value = "workflows/jade-symphony.md")]
    workflow: PathBuf,
    #[arg(long)]
    title: String,
    #[command(flatten)]
    markdown: ForgeMarkdownArgs,
    #[command(flatten)]
    promotion_note: PromotionNoteArgs,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ForgeReworkArgs {
    issue_ref: String,
    #[arg(long, default_value = "workflows/jade-symphony.md")]
    workflow: PathBuf,
    #[arg(long)]
    title: String,
    #[command(flatten)]
    markdown: ForgeMarkdownArgs,
    #[arg(long = "evidence-file")]
    evidence_file: PathBuf,
    #[arg(long = "operator-confirmation")]
    operator_confirmation: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct PromotionNoteArgs {
    #[arg(long = "operator-confirmation")]
    operator_confirmation: String,
    #[arg(long = "decision", required = true)]
    decisions: Vec<String>,
    #[arg(long = "scope-change", required = true)]
    scope_changes: Vec<String>,
    #[arg(long = "dependency-context", required = true)]
    dependencies_context: Vec<String>,
    #[arg(long = "readback-summary")]
    readback_summaries: Vec<String>,
}

#[derive(Debug, Args)]
struct ForgeValidateArgs {
    #[arg(long, default_value = "workflows/jade-symphony.md")]
    workflow: PathBuf,
    #[arg(long, value_enum, ignore_case = true)]
    status: Option<ForgeStatusArg>,
    #[arg(long)]
    title: Option<String>,
    #[command(flatten)]
    markdown: ForgeMarkdownArgs,
    #[arg(long = "issue")]
    issue_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ForgeStatusArg {
    Backlog,
    Todo,
}

impl ForgeStatusArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "Backlog",
            Self::Todo => "Todo",
        }
    }

    fn normalized_state(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Todo => "todo",
        }
    }
}

fn run_loop_command(args: RunLoopArgs) -> Result<Command, String> {
    if args.max_iterations == Some(0) || args.max_concurrent == Some(0) {
        return Err(usage());
    }
    Ok(Command::RunLoop {
        options: RunLoopOptions {
            workflow_path: args.workflow_path,
            max_iterations: args.max_iterations,
            once: args.once,
            write: args.write,
            recover: loop_recover_enabled(args.write, args.recover, args.no_recover),
            max_concurrent: args.max_concurrent,
            display: args.display.into(),
        },
    })
}

fn merge_loop_command(args: MergeLoopArgs) -> Result<Command, String> {
    if args.max_iterations == Some(0) || args.max_concurrent == Some(0) {
        return Err(usage());
    }
    Ok(Command::MergeLoop {
        options: MergeLoopOptions {
            workflow_path: args.workflow_path,
            max_iterations: args.max_iterations,
            once: args.once,
            write: args.write,
            recover: loop_recover_enabled(args.write, args.recover, args.no_recover),
            max_concurrent: args.max_concurrent,
        },
    })
}

fn loop_recover_enabled(write: bool, explicit_recover: bool, no_recover: bool) -> bool {
    explicit_recover || (write && !no_recover)
}

fn command_from_project_args(command: ProjectCommandArgs) -> Result<Command, String> {
    match command {
        ProjectCommandArgs::State(args) => Ok(Command::ProjectState {
            options: ProjectStateOptions {
                workflow_path: args.workflow_path,
                display: args.display.into(),
            },
        }),
        ProjectCommandArgs::Issue(args) => Ok(Command::ProjectIssue {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            json: args.json,
        }),
        ProjectCommandArgs::Inspect(args) => Ok(Command::ProjectInspect {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            lane: args.lane,
        }),
        ProjectCommandArgs::SetState(args) => Ok(Command::SetState {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            state: args.state,
            write: args.write,
        }),
        ProjectCommandArgs::LinkPr(args) => Ok(Command::LinkPr {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            pr_ref: args.pr_ref,
            write: args.write,
        }),
        ProjectCommandArgs::Add(args) => Ok(Command::AddToProject {
            workflow_path: args.workflow_path,
            issue_id: args.issue_id,
            write: args.write,
        }),
        ProjectCommandArgs::Workpad(args) => Ok(Command::Workpad {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            markdown_path: args.markdown_path,
            write: args.write,
        }),
        ProjectCommandArgs::TimelineComment(args) => Ok(Command::TimelineComment {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            markdown_path: args.markdown_path,
            write: args.write,
        }),
    }
}

fn command_from_merge_args(command: MergeCommandArgs) -> Result<Command, String> {
    match command {
        MergeCommandArgs::Claim(claim) => Ok(Command::LaneClaim {
            workflow_path: claim.workflow_path,
            issue_ref: claim.issue_ref,
            lane: AgentSessionLaneArg::Merge,
            worker: claim.worker,
            source: claim.source,
            write: claim.write,
        }),
        MergeCommandArgs::Once(args) => Ok(Command::MergeOnce {
            workflow_path: args.workflow_path,
            write: args.write,
        }),
        MergeCommandArgs::Loop(args) => merge_loop_command(args),
    }
}

impl TryFrom<Cli> for Command {
    type Error = String;

    fn try_from(cli: Cli) -> Result<Self, Self::Error> {
        let default_workflow = || PathBuf::from("WORKFLOW.md");
        match cli.command {
            None => Ok(Self::Plan {
                workflow_path: cli.workflow_path.unwrap_or_else(default_workflow),
                json: false,
            }),
            Some(command) => {
                if cli.workflow_path.is_some() {
                    return Err(usage());
                }

                match command {
                    CliCommand::Plan(args) => Ok(Self::Plan {
                        workflow_path: args.workflow_path,
                        json: args.json,
                    }),
                    CliCommand::Validate(args) => Ok(Self::Validate {
                        workflow_path: args.workflow_path,
                    }),
                    CliCommand::Doctor(args) => Ok(Self::Doctor {
                        options: DoctorOptions {
                            workflow_path: args.workflow_path,
                            json: args.json,
                            strict: args.strict,
                            display: args.display.into(),
                            interactive: args.interactive,
                            auto_fix: args.auto_fix,
                            write: args.write,
                            stale_after_ms: args.stale_after_ms,
                            action: args.action.map(|action| match action {
                                DoctorSubcommandArgs::Repair(repair) => {
                                    DoctorAction::Repair(DoctorRepairIssueOptions {
                                        issue_ref: repair.issue_ref,
                                        write: repair.write,
                                        move_need_human_input: repair.move_need_human_input,
                                        mark_pr_ready: repair.mark_pr_ready,
                                        confirm_handoff_ready: repair.confirm_handoff_ready,
                                    })
                                }
                            }),
                        },
                    }),
                    CliCommand::DoctorRepairHumanReview(args) => {
                        Ok(Self::DoctorRepairHumanReview {
                            workflow_path: args.workflow_path,
                            write: args.write,
                        })
                    }
                    CliCommand::Skills(args) => match args.command {
                        SkillsCommandArgs::Status(args) => Ok(Self::SkillsStatus {
                            input: SkillStatusInput {
                                workflow_path: args.workflow_path,
                                suite_path: args.suite_path,
                                codex_dir: args.codex_dir,
                                gemini_dir: args.gemini_dir,
                                require_gemini: args.require_gemini,
                                session_skills: args.session_skills,
                                session_skills_file: args.session_skills_file,
                            },
                            json: args.json,
                        }),
                    },
                    CliCommand::Profiles(args) => Ok(Self::Profiles {
                        workflow_path: args.workflow_path,
                    }),
                    CliCommand::Debug(args) => Ok(Self::Debug {
                        workflow_path: args.workflow_path,
                    }),
                    CliCommand::Autopilot(args) => match args.command {
                        AutopilotCommandArgs::Plan(args) => Ok(Self::AutopilotPlan {
                            workflow_path: args.workflow_path,
                            json: args.json,
                        }),
                    },
                    CliCommand::Status(args) => match args.command {
                        StatusCommandArgs::Show(show) => Ok(Self::Plan {
                            workflow_path: show.workflow_path,
                            json: show.json,
                        }),
                        StatusCommandArgs::Serve(serve) => Ok(Self::StatusApi {
                            workflow_path: serve.workflow_path,
                            bind: serve.bind,
                            once: serve.once,
                        }),
                    },
                    CliCommand::Clean(args) => match args.command {
                        CleanCommand::Plan(plan) => Ok(Self::CleanPlan {
                            workflow_path: plan.workflow_path,
                        }),
                        CleanCommand::Audit(audit) => Ok(Self::CleanAudit {
                            workflow_path: audit.workflow_path,
                        }),
                    },
                    CliCommand::Workspace(args) => match args.command {
                        WorkspaceCommandArgs::List(list) => Ok(Self::WorkspaceList {
                            workflow_path: list.workflow_path,
                        }),
                        WorkspaceCommandArgs::Show(show) => Ok(Self::WorkspaceShow {
                            workflow_path: show.workflow_path,
                            issue_ref: show.issue_ref,
                        }),
                        WorkspaceCommandArgs::Adopt(adopt) => Ok(Self::WorkspaceAdopt {
                            workflow_path: adopt.workflow_path,
                            issue_ref: adopt.issue_ref,
                            path: adopt.path,
                            write: adopt.write,
                        }),
                        WorkspaceCommandArgs::Ensure(ensure) => Ok(Self::WorkspaceEnsure {
                            workflow_path: ensure.workflow_path,
                            issue_ref: ensure.issue_ref,
                            pr_ref: ensure.pr_ref,
                            branch: ensure.branch,
                            write: ensure.write,
                        }),
                    },
                    CliCommand::Project(args) => command_from_project_args(args.command),
                    CliCommand::Main(args) => lane_command(AgentSessionLaneArg::Main, args),
                    CliCommand::Merge(args) => command_from_merge_args(args.command),
                    CliCommand::Session(args) => match args.command {
                        SessionCommand::Start(start) => Ok(Self::SessionStart {
                            workflow_path: start.workflow_path,
                            issue_ref: start.issue_ref,
                            lane: start.lane,
                            run_id: start.run_id,
                            write: start.write,
                        }),
                        SessionCommand::List(list) => Ok(Self::SessionList {
                            workflow_path: list.workflow_path,
                        }),
                        SessionCommand::Attach(attach) => Ok(Self::SessionAttach {
                            workflow_path: attach.workflow_path,
                            session: attach.session,
                            exec: attach.exec,
                        }),
                    },
                    CliCommand::CreateFollowUp(args) => Ok(Self::CreateFollowUp {
                        workflow_path: args.workflow,
                        title: args.title,
                        body_path: args.body_file,
                        write: args.write,
                    }),
                    CliCommand::Review(args) => command_from_review_args(args.command),
                    CliCommand::Forge(args) => match args.command {
                        ForgeCommandArgs::Create(args) => Ok(Self::ForgeCreate {
                            workflow_path: args.workflow,
                            title: args.title,
                            markdown: read_forge_markdown_arg(args.markdown)?,
                            status: args.status,
                            project: args.project,
                            project_fields: parse_project_field_assignments(args.project_fields)?,
                            assignees: args.assignees,
                            write: args.write,
                            dry_run: args.dry_run,
                        }),
                        ForgeCommandArgs::Promote(args) => Ok(Self::ForgePromote {
                            workflow_path: args.workflow,
                            issue_ref: args.issue_ref,
                            title: args.title,
                            markdown: read_forge_markdown_arg(args.markdown)?,
                            promotion_note: promotion_note_input(args.promotion_note)?,
                            write: args.write,
                            dry_run: args.dry_run,
                        }),
                        ForgeCommandArgs::Rework(args) => Ok(Self::ForgeRework {
                            options: ForgeReworkOptions {
                                workflow_path: args.workflow,
                                issue_ref: args.issue_ref,
                                title: args.title,
                                markdown: read_forge_markdown_arg(args.markdown)?,
                                evidence: read_required_file(args.evidence_file)?,
                                operator_confirmation: args.operator_confirmation,
                                write: args.write,
                                dry_run: args.dry_run,
                            },
                        }),
                        ForgeCommandArgs::Validate(args) => {
                            if let Some(issue_ref) = args.issue_ref {
                                Ok(Self::ForgeValidate {
                                    workflow_path: args.workflow,
                                    status: args.status,
                                    title: args.title.unwrap_or_default(),
                                    markdown: read_optional_forge_markdown_arg(args.markdown)?,
                                    issue_ref: Some(issue_ref),
                                })
                            } else {
                                Ok(Self::ForgeValidate {
                                    workflow_path: args.workflow,
                                    status: args.status,
                                    title: args.title.ok_or(
                                        "forge validate requires --title when --issue is not used",
                                    )?,
                                    markdown: read_forge_markdown_arg(args.markdown)?,
                                    issue_ref: None,
                                })
                            }
                        }
                    },
                    CliCommand::Run => {
                        Err("`jade-symphony run` is reserved for future all-lane orchestration and is not implemented yet".into())
                    }
                    CliCommand::Upgrade => {
                        Err("`jade-symphony upgrade` is reserved for future Jade Symphony binary and skill upgrades and is not implemented yet".into())
                    }
                }
            }
        }
    }
}

fn gate_workpad(issue: &TrackerIssue, decision: &GateDecision) -> String {
    let mut lines = vec![
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Context".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Current state: {}", issue.state),
        String::new(),
        "### Decisions / Assumptions".to_string(),
    ];

    if decision.assumptions.is_empty() {
        lines.push("- None recorded.".into());
    } else {
        lines.extend(decision.assumptions.iter().map(|item| format!("- {item}")));
    }

    lines.extend([
        String::new(),
        "### Quality Gate".to_string(),
        format!("- Decision: {:?}", decision.kind),
    ]);

    if !decision.missing.is_empty() {
        lines.push(format!("- Missing: {}", decision.missing.join(", ")));
    }
    if !decision.notes.is_empty() {
        lines.extend(decision.notes.iter().map(|item| format!("- Note: {item}")));
    }

    lines.extend([
        String::new(),
        "### Plan".to_string(),
        "- [ ] Resolve quality-gate findings before dispatch.".to_string(),
        String::new(),
        "### Validation".to_string(),
        "- [ ] Re-run `jade-symphony forge validate --issue` after issue updates.".to_string(),
    ]);

    lines.join("\n")
}

fn gate_target_state(decision: &GateDecision) -> &'static str {
    match decision.kind {
        GateDecisionKind::NeedToClarify | GateDecisionKind::TooBroad => "need_to_clarify",
        GateDecisionKind::Blocked => "need_human_input",
        GateDecisionKind::DuplicateAlreadyCovered => "done",
        GateDecisionKind::Ready | GateDecisionKind::ReadyWithAssumptions => "todo",
    }
}

fn read_forge_markdown_arg(args: ForgeMarkdownArgs) -> Result<String, String> {
    match (args.body, args.body_file) {
        (Some(value), None) => Ok(value),
        (None, Some(path)) => std::fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display())),
        _ => Err(usage()),
    }
}

fn read_optional_forge_markdown_arg(args: ForgeMarkdownArgs) -> Result<String, String> {
    match (args.body, args.body_file) {
        (Some(value), None) => Ok(value),
        (None, Some(path)) => std::fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display())),
        (None, None) => Ok(String::new()),
        (Some(_), Some(_)) => Err(usage()),
    }
}

fn read_required_file(path: PathBuf) -> Result<String, String> {
    std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))
}

fn command_from_review_args(command: ReviewCommandArgs) -> Result<Command, String> {
    match command {
        ReviewCommandArgs::Fake(args) => Ok(Command::ReviewFake {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            outcome: args.outcome.into(),
            write: args.write,
        }),
        ReviewCommandArgs::Once(args) => Ok(Command::ReviewOnce {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            write: args.write,
        }),
        ReviewCommandArgs::Claim(args) => Ok(Command::LaneClaim {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            lane: AgentSessionLaneArg::Review,
            worker: args.worker,
            source: args.source,
            write: args.write,
        }),
        ReviewCommandArgs::Pass(args) => Ok(Command::ReviewPass {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            evidence: read_required_file(args.evidence_file)?,
            write: args.write,
        }),
        ReviewCommandArgs::Reject(args) => Ok(Command::ReviewReject {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            evidence: read_required_file(args.evidence_file)?,
            target_state: args.target_state,
            write: args.write,
        }),
        ReviewCommandArgs::Session(args) => Ok(Command::ReviewSession {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            write: args.write,
        }),
        ReviewCommandArgs::Freshness(args) => Ok(Command::ReviewFreshness {
            input: ReviewFreshnessInput {
                issue_ref: args.issue_ref,
                prior_head_sha: args.prior_head_sha,
                current_head_sha: args.current_head_sha,
                prior_base_sha: args.prior_base_sha,
                current_base_sha: args.current_base_sha,
                changed_files: args.changed_files,
                stale_reason: args.stale_reason.into(),
                rework_class: args.rework_class.into(),
                patch_summary: args.patch_summary,
            },
        }),
        ReviewCommandArgs::Loop(args) => {
            if args.max_iterations == Some(0) || args.max_concurrent == Some(0) {
                return Err(usage());
            }
            Ok(Command::ReviewLoop {
                options: ReviewLoopOptions {
                    workflow_path: args.workflow_path,
                    max_iterations: args.max_iterations,
                    once: args.once,
                    write: args.write,
                    fake_outcome: args.fake_outcome.map(Into::into),
                    max_concurrent: args.max_concurrent,
                },
            })
        }
        ReviewCommandArgs::Status(args) => {
            if args.recent_limit == 0 {
                return Err(usage());
            }
            Ok(Command::ReviewStatus {
                options: ReviewStatusCliOptions {
                    workflow_path: args.workflow_path,
                    issue_filter: args.issue_filter,
                    recent_limit: args.recent_limit,
                    verbose: args.verbose,
                    json: args.json,
                },
            })
        }
    }
}

fn parse_project_field_assignments(
    values: Vec<String>,
) -> Result<Vec<ProjectFieldAssignment>, String> {
    values
        .into_iter()
        .map(|value| ProjectFieldAssignment::parse(&value).map_err(|error| error.to_string()))
        .collect()
}

fn promotion_note_input(args: PromotionNoteArgs) -> Result<PromotionNoteInput, String> {
    fn clean_nonempty(value: String, field: &str) -> Result<String, String> {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            Err(format!("forge promote requires non-empty {field}"))
        } else {
            Ok(trimmed)
        }
    }

    fn clean_many(values: Vec<String>, field: &str) -> Result<Vec<String>, String> {
        let cleaned = values
            .into_iter()
            .map(|value| clean_nonempty(value, field))
            .collect::<Result<Vec<_>, _>>()?;
        if cleaned.is_empty() {
            Err(format!("forge promote requires at least one {field}"))
        } else {
            Ok(cleaned)
        }
    }

    Ok(PromotionNoteInput {
        operator_confirmation: clean_nonempty(
            args.operator_confirmation,
            "--operator-confirmation",
        )?,
        decisions: clean_many(args.decisions, "--decision")?,
        scope_changes: clean_many(args.scope_changes, "--scope-change")?,
        dependencies_context: clean_many(args.dependencies_context, "--dependency-context")?,
        readback_summaries: args
            .readback_summaries
            .into_iter()
            .map(|value| clean_nonempty(value, "--readback-summary"))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn print_forge_validation(report: &ForgeValidationReport) {
    let categories = forge_missing_categories(report);
    println!("title={}", report.title);
    println!("gate={:?}", report.decision.kind);
    println!("dispatchable={}", report.decision.is_dispatchable());
    if !report.decision.missing.is_empty() {
        println!("missing={}", report.decision.missing.join(", "));
    }
    println!(
        "candidate_missing={}",
        missing_category_value(&categories.candidate_missing)
    );
    println!(
        "live_context_missing={}",
        missing_category_value(&categories.live_context_missing)
    );
    if !report.decision.assumptions.is_empty() {
        println!("assumptions={}", report.decision.assumptions.join("; "));
    }
    if let Some(question) = &report.question {
        println!("question={}", question.question);
        println!("why={}", question.why_it_matters);
    }
}

fn missing_category_value(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}

fn usage() -> String {
    [
        "OpenAI Symphony-style orchestration harness with Jade Symphony extensions",
        "",
        "Usage: jade-symphony [path-to-WORKFLOW.md] [COMMAND]",
        "",
        "Human / Operator operations:",
        "  plan                        Render the dispatch/status plan",
        "  validate                    Validate workflow loading and configuration",
        "  doctor                      Audit Project, workflow, and runtime invariants",
        "  skills                      Inspect per-repo skill readiness",
        "  status                      Show or serve runtime status snapshots",
        "  clean                       Plan or audit artifact cleanup",
        "  profiles                    List execution profiles",
        "  debug                       Render a combined operator debug report",
        "",
        "Project / Agent internals:",
        "  project                     Read or mutate Project facts through grouped subcommands",
        "  workspace                   Discover and record per-issue git worktrees",
        "  session                     Start, list, or attach supervised lane sessions",
        "",
        "Lane orchestration:",
        "  autopilot                   Read-only all-lane planning preflight",
        "  main                        Main Agent claim, once, and loop commands",
        "  review                      Review Agent claim, pass/reject, session, freshness, and loop commands",
        "  merge                       Merging Agent claim, once, and loop commands",
        "  create-follow-up            Create an operator follow-up issue",
        "",
        "Issue Forge:",
        "  forge                       Validate, create, or promote issue contracts",
        "",
        "Reserved lifecycle topology:",
        "  run                         Reserved for future all-lane automatic orchestration",
        "  upgrade                     Reserved for future Jade Symphony binary and skill upgrades",
        "",
        "Arguments:",
        "  [path-to-WORKFLOW.md]",
        "",
        "Options:",
        "  -h, --help                  Print help",
        "",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jade_symphony::tracker::MemoryTracker;
    use std::cell::RefCell;

    fn forge_contract() -> String {
        [
            "## Issue Setup",
            "- UAT Required: No",
            "## Issue Goal",
            "Create a validated tracker issue.",
            "## Why Now",
            "Now.",
            "## Issue Context",
            "Context.",
            "## Dependencies",
            "- No blocking dependencies.",
            "## Non-Negotiable Guardrails",
            "- Guard.",
            "## Scope",
            "Scope.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/jade-symphony",
            "## Verification",
            "### Completion Criteria",
            "- Pass.",
            "### Functional Verification",
            "- `cargo test`",
        ]
        .join("\n")
    }

    fn parse(args: &[&str]) -> Command {
        Command::parse(args.iter().map(|arg| arg.to_string()).collect()).unwrap()
    }

    fn git_ok(path: &Path, args: &[&str]) {
        let output = ProcessCommand::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap_or_else(|error| panic!("failed to run git {args:?}: {error}"));
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout={}\nstderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn canonical_git_repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let remote = temp.path().join("origin.git");
        let repo = temp.path().join("repo");
        git_ok(
            temp.path(),
            &["init", "--bare", "--initial-branch=main", "origin.git"],
        );
        git_ok(temp.path(), &["init", "--initial-branch=main", "repo"]);
        git_ok(&repo, &["config", "user.email", "jade@example.invalid"]);
        git_ok(&repo, &["config", "user.name", "Jade Symphony"]);
        std::fs::write(repo.join("README.md"), "main\n").unwrap();
        git_ok(&repo, &["add", "README.md"]);
        git_ok(&repo, &["commit", "-m", "initial"]);
        git_ok(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_ok(&repo, &["push", "-u", "origin", "main"]);
        (temp, repo, remote)
    }

    #[test]
    fn canonical_checkout_report_accepts_latest_main() {
        let (_temp, repo, _remote) = canonical_git_repo();

        assert_eq!(
            canonical_checkout_report(&repo),
            CanonicalCheckoutReport::Ready
        );
    }

    #[test]
    fn canonical_checkout_report_blocks_detached_head() {
        let (_temp, repo, _remote) = canonical_git_repo();
        git_ok(&repo, &["checkout", "--detach", "HEAD"]);

        let report = canonical_checkout_report(&repo);
        assert!(matches!(
            report,
            CanonicalCheckoutReport::Blocked { ref reason } if reason.contains("detached")
        ));
    }

    #[test]
    fn canonical_checkout_report_blocks_non_main_branch() {
        let (_temp, repo, _remote) = canonical_git_repo();
        git_ok(&repo, &["checkout", "-b", "feature/test"]);

        let report = canonical_checkout_report(&repo);
        assert!(matches!(
            report,
            CanonicalCheckoutReport::Blocked { ref reason } if reason.contains("current branch")
        ));
    }

    #[test]
    fn canonical_checkout_report_blocks_main_behind_origin_main() {
        let (temp, repo, remote) = canonical_git_repo();
        let other = temp.path().join("other");
        git_ok(
            temp.path(),
            &["clone", remote.to_str().unwrap(), other.to_str().unwrap()],
        );
        git_ok(&other, &["config", "user.email", "jade@example.invalid"]);
        git_ok(&other, &["config", "user.name", "Jade Symphony"]);
        std::fs::write(other.join("CHANGELOG.md"), "change\n").unwrap();
        git_ok(&other, &["add", "CHANGELOG.md"]);
        git_ok(&other, &["commit", "-m", "advance main"]);
        git_ok(&other, &["push", "origin", "main"]);

        let report = canonical_checkout_report(&repo);
        assert!(matches!(
            report,
            CanonicalCheckoutReport::Blocked { ref reason }
                if reason.contains("local main does not exactly match origin/main")
        ));
    }

    #[test]
    fn tracker_mutation_audit_records_logical_actor_identity() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        config.identity.actor_role = "merge_agent".into();
        config.identity.actor_label = "Jade Symphony Merge Worker".into();

        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "merge once",
                mutation_type: "state_change",
                issue_ref: Some("#7"),
                target: Some("https://github.com/Alive24/jade-symphony/pull/7".into()),
                from_state: Some("Merging".into()),
                to_state: Some("Done".into()),
                reason: "merge completed",
            },
        );

        let records = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"))
            .read_records()
            .unwrap();
        let record = records.first().expect("expected audit record");
        assert_eq!(record.event, "tracker_mutation");
        assert_eq!(record.actor_role.as_deref(), Some("merge_agent"));
        assert_eq!(
            record.actor_label.as_deref(),
            Some("Jade Symphony Merge Worker")
        );
        assert_eq!(
            record
                .tracker_mutation
                .as_ref()
                .map(|audit| audit.mutation_type.as_str()),
            Some("state_change")
        );
    }

    #[test]
    fn manual_lane_claim_evidence_records_non_tmux_registry_records() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.artifacts.namespace = Some("acme/project".into());
        let issue = tracker_issue_with_ref("#281", "Manual evidence", "In Progress");

        for (index, lane) in [
            AgentSessionLaneArg::Main,
            AgentSessionLaneArg::Review,
            AgentSessionLaneArg::Merge,
        ]
        .into_iter()
        .enumerate()
        {
            let worker = format!("codex-manual-{}", lane.label());
            let claim = LaneClaim::active(
                &issue.identifier,
                lane.claim_lane(),
                LaneClaimActor::Codex,
                LaneClaimSource::Manual,
                1_779_000_900_123 + index as u64,
            )
            .with_worker(&worker);
            let claim_value = claim.render();

            record_manual_lane_claim_evidence(&config, &issue, lane, &claim, &claim_value, &worker)
                .unwrap();
        }

        let registry = load_session_registry(&session_registry_path(&config)).unwrap();
        assert_eq!(registry.sessions.len(), 3);
        for record in registry.sessions {
            assert_eq!(record.issue_identifier.as_deref(), Some("#281"));
            assert_eq!(record.backend, "codex-app-manual");
            assert_eq!(record.status, SessionStatus::Recorded);
            assert_eq!(record.session_source.as_deref(), Some("manual-claim"));
            assert_eq!(record.thread.as_deref(), Some("unknown"));
            assert!(record
                .claim_value
                .as_deref()
                .unwrap()
                .contains("source=manual"));
            assert!(record.pane_target.is_empty());
            assert_eq!(
                record.attach_command,
                "not a tmux session; manual Codex App evidence only"
            );
        }
    }

    #[test]
    fn execute_issue_stores_rendered_prompt_outside_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_path = temp.path().join("WORKFLOW.md");
        let workspace_root = temp.path().join("worktrees");
        let logs_root = temp.path().join("logs");
        let workflow = WorkflowDefinition::parse(
            &workflow_path,
            &format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {:?}\nobservability:\n  logs_root: {:?}\n---\nPrompt for {{{{ issue.identifier }}}}",
                workspace_root.display().to_string(),
                logs_root.display().to_string()
            ),
        )
        .unwrap();
        let config = RuntimeConfig::from_workflow(&workflow, &workflow_path).unwrap();
        let issue = tracker_issue("Todo");

        let result =
            execute_issue_once_with_workspace_key(&workflow, &config, &issue, "issue-29", 3, None)
                .unwrap();

        assert!(!result
            .workspace_path
            .join("JADE_SYMPHONY_PROMPT.md")
            .exists());
        let prompt_path = result
            .prompt_artifact_path
            .expect("expected prompt artifact path");
        assert!(prompt_path.starts_with(logs_root.join("prompts")));
        assert!(prompt_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains("29") && name.contains("attempt-3")));
        assert_eq!(
            std::fs::read_to_string(&prompt_path).unwrap(),
            "Prompt for #29"
        );

        let records = EventLog::new(logs_root.join("jade-symphony.jsonl"))
            .read_records()
            .unwrap();
        assert!(records.iter().any(|record| {
            record.event == "prompt_artifact"
                && record.message.contains(&prompt_path.display().to_string())
        }));
    }

    #[test]
    fn temporary_workflow_paths_emit_operator_warning() {
        let warning =
            temporary_workflow_warning(Path::new("/private/tmp/jade-github-project-workflow.md"))
                .expect("expected temporary workflow warning");

        assert!(warning.contains("workflow_warning=temporary_path"));
        assert!(warning.contains("action=promote"));
        assert!(
            temporary_workflow_warning(Path::new("examples/github-project-workflow.md")).is_none()
        );
    }

    fn help_text(args: &[&str]) -> String {
        let Command::Help(text) = parse(args) else {
            panic!("expected help command");
        };
        text
    }

    fn test_config() -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\n---\nPrompt",
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn main_loop_test_config() -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\n  active_states:\n    - Todo\n    - Rework\n  terminal_states:\n    - Done\n---\nPrompt",
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn live_github_config(allow_unassigned: bool) -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\n  assignee_filter:\n    allow_unassigned: {}\n---\nPrompt",
                allow_unassigned
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn fixture_github_config() -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\n  fixture_path: fixtures/dry-run-issues.json\n---\nPrompt",
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn tracker_issue(state: &str) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: "ISSUE_29".into(),
            item_id: None,
            identifier: "#29".into(),
            title: "Wire runtime state persistence into main loop".into(),
            description: None,
            url: None,
            state: state.into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: Some("feature/issue-29-runtime-state-main-loop".into()),
            linked_pull_requests: Vec::new(),
            blocked_by: Vec::new(),
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    fn test_claim(issue: &TrackerIssue) -> LaneClaim {
        LaneClaim::active(
            &issue.identifier,
            LaneClaimLane::Main,
            LaneClaimActor::Codex,
            LaneClaimSource::Loop,
            1_779_000_900_123,
        )
    }

    fn tracker_issue_with_ref(identifier: &str, title: &str, state: &str) -> TrackerIssue {
        let mut issue = tracker_issue(state);
        issue.identifier = identifier.into();
        issue.title = title.into();
        issue.branch_name = None;
        issue
    }

    fn clean_autopilot_canonical() -> AutopilotCanonicalCheckout {
        AutopilotCanonicalCheckout {
            safe_for_write: true,
            root: Some("/repo".into()),
            branch: Some("main".into()),
            upstream: Some("origin/main".into()),
            clean: Some(true),
            reason: None,
            status_line: Some("canonical_checkout root=/repo branch=main upstream=origin/main clean=true tracked_dirty=0 untracked=0 unclassified=0 migrated=0 quarantine=/repo/.tmp".into()),
        }
    }

    fn clean_autopilot_runtime() -> AutopilotRuntimeSummary {
        AutopilotRuntimeSummary {
            runtime_state_count: 0,
            session_count: 0,
            session_attention_count: 0,
            blockers: Vec::new(),
            evidence: Vec::new(),
        }
    }

    fn clean_autopilot_doctor(total_issues: usize) -> ProjectAuditReport {
        ProjectAuditReport {
            total_issues,
            violations: Vec::new(),
            integration_gaps: Vec::new(),
            skill_readiness_summary: None,
        }
    }

    fn test_autopilot_plan(issues: Vec<TrackerIssue>) -> AutopilotPlanSnapshot {
        let config = test_config();
        let adapter = jade_symphony::tracker::MemoryTracker::new(issues.clone());
        build_autopilot_plan_from_parts(AutopilotPlanInputs {
            workflow_path: Path::new("/tmp/WORKFLOW.md"),
            config: &config,
            adapter: &adapter,
            issues: issues.clone(),
            doctor_report: clean_autopilot_doctor(issues.len()),
            canonical_checkout: clean_autopilot_canonical(),
            runtime: clean_autopilot_runtime(),
            integration_gaps: Vec::new(),
        })
        .unwrap()
    }

    #[test]
    fn autopilot_plan_reports_all_lanes_idle() {
        let plan = test_autopilot_plan(Vec::new());

        assert_eq!(plan.readiness.status, "idle_but_healthy");
        assert_eq!(
            plan.lanes
                .iter()
                .map(|lane| (lane.lane.as_str(), lane.reason.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("main", "no_dispatchable_issue"),
                ("review", "no_agent_review_issue"),
                ("merge", "no_merging_issue")
            ]
        );
        assert!(plan.lanes.iter().all(|lane| lane.selected_issue.is_none()));
    }

    #[test]
    fn parses_grouped_autopilot_plan_json_command() {
        let Command::AutopilotPlan {
            workflow_path,
            json,
        } = parse(&["autopilot", "plan", "workflows/jade-symphony.md", "--json"])
        else {
            panic!("expected autopilot plan command");
        };

        assert_eq!(workflow_path, PathBuf::from("workflows/jade-symphony.md"));
        assert!(json);
    }

    #[test]
    fn autopilot_plan_reports_merge_ready_issue() {
        let mut issue = tracker_issue_with_ref("#338", "Ready merge", "Merging");
        issue
            .linked_pull_requests
            .push(jade_symphony::model::LinkedPullRequest {
                number: Some(339),
                url: Some("https://github.com/Alive24/jade-symphony/pull/339".into()),
                state: Some("OPEN".into()),
                is_draft: Some(false),
                merge_state_status: Some("CLEAN".into()),
                review_decision: Some("APPROVED".into()),
                base_ref_name: Some("main".into()),
                head_ref_name: Some("feature/issue-338".into()),
                ..Default::default()
            });

        let plan = test_autopilot_plan(vec![issue]);
        let merge = plan.lanes.iter().find(|lane| lane.lane == "merge").unwrap();

        assert_eq!(plan.readiness.status, "ready");
        assert_eq!(merge.status, "ready");
        assert_eq!(merge.proposed_action, "merge_pull_request");
        assert_eq!(merge.target_state.as_deref(), Some("done"));
        assert_eq!(
            merge
                .selected_issue
                .as_ref()
                .map(|issue| issue.identifier.as_str()),
            Some("#338")
        );
    }

    #[test]
    fn autopilot_plan_does_not_mutate_tracker_adapter() {
        let config = test_config();
        let issue = tracker_issue_with_ref("#338", "Ready merge", "Merging");
        let adapter = RecordingAdapter::default();
        adapter
            .linked_pull_requests
            .borrow_mut()
            .push(jade_symphony::model::LinkedPullRequest {
                number: Some(339),
                url: Some("https://github.com/Alive24/jade-symphony/pull/339".into()),
                state: Some("OPEN".into()),
                is_draft: Some(false),
                merge_state_status: Some("CLEAN".into()),
                review_decision: Some("APPROVED".into()),
                base_ref_name: Some("main".into()),
                head_ref_name: Some("feature/issue-338".into()),
                ..Default::default()
            });

        let plan = build_autopilot_plan_from_parts(AutopilotPlanInputs {
            workflow_path: Path::new("/tmp/WORKFLOW.md"),
            config: &config,
            adapter: &adapter,
            issues: vec![issue],
            doctor_report: clean_autopilot_doctor(1),
            canonical_checkout: clean_autopilot_canonical(),
            runtime: clean_autopilot_runtime(),
            integration_gaps: Vec::new(),
        })
        .unwrap();

        assert!(plan.read_only);
        assert_eq!(plan.readiness.status, "ready");
        assert!(adapter.operations().is_empty());
    }

    #[test]
    fn autopilot_plan_reports_parked_operator_queues() {
        let human_review = tracker_issue_with_ref("#41", "Needs human approval", "Human Review");
        let need_human_input =
            tracker_issue_with_ref("#42", "Needs operator decision", "Need Human Input");

        let plan = test_autopilot_plan(vec![human_review, need_human_input]);

        let human_queue = plan
            .parked_queues
            .iter()
            .find(|queue| queue.name == "Human Review")
            .unwrap();
        let input_queue = plan
            .parked_queues
            .iter()
            .find(|queue| queue.name == "Need Human Input")
            .unwrap();
        assert_eq!(human_queue.count, 1);
        assert_eq!(input_queue.count, 1);
        assert!(plan.lanes.iter().all(|lane| lane.selected_issue.is_none()));
    }

    #[test]
    fn autopilot_plan_blocks_on_doctor_or_canonical_checkout() {
        let config = test_config();
        let issues = Vec::new();
        let adapter = jade_symphony::tracker::MemoryTracker::new(issues.clone());
        let doctor = ProjectAuditReport {
            total_issues: 0,
            violations: vec![ProjectAuditViolation {
                issue_ref: "canonical".into(),
                title: "Canonical checkout has tracked dirty files".into(),
                state: "local".into(),
                severity: AuditSeverity::Blocker,
                code: "canonical_checkout_tracked_dirty".into(),
                message: "tracked dirty files".into(),
                suggestion: "clean checkout".into(),
            }],
            integration_gaps: Vec::new(),
            skill_readiness_summary: None,
        };
        let canonical = AutopilotCanonicalCheckout {
            safe_for_write: false,
            reason: Some("current branch is \"feature/test\", expected \"main\"".into()),
            ..clean_autopilot_canonical()
        };

        let plan = build_autopilot_plan_from_parts(AutopilotPlanInputs {
            workflow_path: Path::new("/tmp/WORKFLOW.md"),
            config: &config,
            adapter: &adapter,
            issues,
            doctor_report: doctor,
            canonical_checkout: canonical,
            runtime: clean_autopilot_runtime(),
            integration_gaps: Vec::new(),
        })
        .unwrap();

        assert_eq!(
            plan.readiness.status,
            "blocked_by_doctor_or_canonical_checkout"
        );
        assert!(plan
            .readiness
            .blockers
            .iter()
            .any(|blocker| blocker.contains("doctor_blockers=1")));
        assert!(plan
            .readiness
            .blockers
            .iter()
            .any(|blocker| blocker.contains("canonical_checkout=")));
    }

    #[test]
    fn autopilot_plan_does_not_select_non_dispatchable_or_parked_states() {
        let mut dogfood = tracker_issue_with_ref("#330", "Dogfood session coordination", "Backlog");
        dogfood.labels.push("dogfood-session".into());
        let mut todo_dogfood = tracker_issue_with_ref("#335", "Dogfood: live lane run", "Todo");
        todo_dogfood.labels.push("dogfood-session".into());
        let issues = vec![
            dogfood,
            todo_dogfood,
            tracker_issue_with_ref("#331", "Done main lane", "Done"),
            tracker_issue_with_ref("#332", "Human parked", "Human Review"),
            tracker_issue_with_ref("#333", "Needs input", "Need Human Input"),
            tracker_issue_with_ref("#334", "Clarify me", "Need to Clarify"),
        ];

        let plan = test_autopilot_plan(issues);

        assert!(plan.lanes.iter().all(|lane| lane.selected_issue.is_none()));
        assert_eq!(
            plan.parked_queues
                .iter()
                .find(|queue| queue.name == "Dogfood / Coordination")
                .unwrap()
                .issues
                .iter()
                .map(|issue| issue.identifier.as_str())
                .collect::<Vec<_>>(),
            vec!["#330", "#335"]
        );
    }

    fn tracker_issue_with_review_claim() -> TrackerIssue {
        let mut issue = tracker_issue("Agent Review");
        let claim = LaneClaim::active(
            &issue.identifier,
            LaneClaimLane::Review,
            LaneClaimActor::Gemini,
            LaneClaimSource::Manual,
            1_779_000_900_123,
        );
        issue.project_fields.insert(
            "Review Agent".into(),
            serde_json::Value::String(claim.render()),
        );
        issue
    }

    fn review_issue_with_ref(identifier: &str, title: &str) -> TrackerIssue {
        let mut issue = tracker_issue_with_ref(identifier, title, "Agent Review");
        let number = identifier.trim_start_matches('#');
        issue
            .linked_pull_requests
            .push(jade_symphony::model::LinkedPullRequest {
                number: number.parse().ok(),
                url: Some(format!(
                    "https://github.com/Alive24/jade-symphony/pull/{number}"
                )),
                state: Some("OPEN".into()),
                is_draft: Some(false),
                ..Default::default()
            });
        issue
    }

    struct RecordingAdapter {
        operations: RefCell<Vec<String>>,
        issues: RefCell<BTreeMap<String, TrackerIssue>>,
        hydrated_issues: RefCell<Vec<String>>,
        linked_pull_requests: RefCell<Vec<jade_symphony::model::LinkedPullRequest>>,
        fail_workpad: bool,
        fail_comment: bool,
        fail_link_pr: bool,
        fail_state_after_apply: bool,
        fail_project_field_after_apply: bool,
        fail_workpad_after_apply: bool,
        fail_comment_after_apply: bool,
        fail_close_after_apply: bool,
        confirm_link_pr: bool,
        fail_get_issue: bool,
    }

    impl Default for RecordingAdapter {
        fn default() -> Self {
            Self {
                operations: RefCell::new(Vec::new()),
                issues: RefCell::new(BTreeMap::new()),
                hydrated_issues: RefCell::new(Vec::new()),
                linked_pull_requests: RefCell::new(Vec::new()),
                fail_workpad: false,
                fail_comment: false,
                fail_link_pr: false,
                fail_state_after_apply: false,
                fail_project_field_after_apply: false,
                fail_workpad_after_apply: false,
                fail_comment_after_apply: false,
                fail_close_after_apply: false,
                confirm_link_pr: true,
                fail_get_issue: false,
            }
        }
    }

    impl RecordingAdapter {
        fn operations(&self) -> Vec<String> {
            self.operations.borrow().clone()
        }
    }

    impl TrackerAdapter for RecordingAdapter {
        fn kind(&self) -> &'static str {
            "recording"
        }

        fn list_dispatchable_issues(
            &self,
        ) -> Result<Vec<TrackerIssue>, jade_symphony::tracker::TrackerError> {
            Ok(Vec::new())
        }

        fn get_issue(
            &self,
            issue_ref: &str,
        ) -> Result<Option<TrackerIssue>, jade_symphony::tracker::TrackerError> {
            if self.fail_get_issue {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "simulated get_issue failure".into(),
                    ),
                );
            }
            Ok(self.issues.borrow().get(issue_ref).cloned())
        }

        fn hydrate_issue_evidence(
            &self,
            mut issue: TrackerIssue,
            _project_context: &[TrackerIssue],
        ) -> Result<TrackerIssue, jade_symphony::tracker::TrackerError> {
            self.hydrated_issues
                .borrow_mut()
                .push(issue.identifier.clone());
            issue.description = Some(format!("rich evidence for {}", issue.identifier));
            Ok(issue)
        }

        fn fetch_issues_by_states(
            &self,
            _states: &[String],
        ) -> Result<Vec<TrackerIssue>, jade_symphony::tracker::TrackerError> {
            Ok(Vec::new())
        }

        fn set_state(
            &self,
            issue_ref: &str,
            normalized_state: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            if let Some(issue) = self.issues.borrow_mut().get_mut(issue_ref) {
                issue.state = normalize_state(normalized_state);
            }
            self.operations
                .borrow_mut()
                .push(format!("set_state:{issue_ref}:{normalized_state}"));
            if self.fail_state_after_apply {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "GitHub GraphQL operation failed: HTTP 502 Bad Gateway".into(),
                    ),
                );
            }
            Ok(())
        }

        fn upsert_workpad(
            &self,
            issue_ref: &str,
            markdown: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            if self.fail_workpad {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "workpad failed".into(),
                    ),
                );
            }
            assert!(
                markdown.contains("## Jade Symphony Workpad")
                    || markdown.contains("### Workspace Evidence")
            );
            self.operations
                .borrow_mut()
                .push(format!("workpad:{issue_ref}"));
            if let Some(issue) = self.issues.borrow_mut().get_mut(issue_ref) {
                issue.description = Some(markdown.to_string());
            }
            if self.fail_workpad_after_apply {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "GitHub GraphQL operation failed: HTTP 502 Bad Gateway".into(),
                    ),
                );
            }
            Ok(())
        }

        fn update_issue_content(
            &self,
            issue_ref: &str,
            title: &str,
            body: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            if let Some(issue) = self.issues.borrow_mut().get_mut(issue_ref) {
                issue.title = title.to_string();
                issue.description = Some(body.to_string());
            }
            self.operations
                .borrow_mut()
                .push(format!("update_issue_content:{issue_ref}"));
            Ok(())
        }

        fn add_issue_comment(
            &self,
            issue_ref: &str,
            markdown: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            if self.fail_comment {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "comment failed".into(),
                    ),
                );
            }
            assert!(
                markdown.contains("## Promotion Note")
                    || markdown.contains("## Jade Symphony Agent Review Run")
                    || markdown.contains("## Jade Symphony Rework Run")
                    || markdown.contains("## Jade Symphony Merge Run")
                    || markdown.contains("## Jade Symphony Doctor Triage")
            );
            self.operations
                .borrow_mut()
                .push(format!("comment:{issue_ref}"));
            if let Some(issue) = self.issues.borrow_mut().get_mut(issue_ref) {
                let mut description = issue.description.clone().unwrap_or_default();
                if !description.is_empty() {
                    description.push_str("\n\n");
                }
                description.push_str(markdown);
                issue.description = Some(description);
            }
            if self.fail_comment_after_apply {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "GitHub GraphQL operation failed: HTTP 502 Bad Gateway".into(),
                    ),
                );
            }
            Ok(())
        }

        fn create_follow_up_issue(
            &self,
            input: FollowUpIssueInput,
        ) -> Result<String, jade_symphony::tracker::TrackerError> {
            let issue_id = format!("dry-run:{}", input.title);
            let mut issue = tracker_issue_with_ref(&issue_id, &input.title, "untriaged");
            issue.id = issue_id.clone();
            issue.description = Some(input.body);
            issue.assignees = input.assignees;
            self.issues.borrow_mut().insert(issue_id.clone(), issue);
            self.operations
                .borrow_mut()
                .push(format!("create_issue:{issue_id}"));
            Ok(issue_id)
        }

        fn add_issue_to_project(
            &self,
            issue_id: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            self.add_issue_to_project_with_state(issue_id, "todo")
        }

        fn add_issue_to_project_with_state(
            &self,
            issue_id: &str,
            normalized_state: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            let normalized_state = normalize_state(normalized_state);
            if let Some(issue) = self.issues.borrow_mut().get_mut(issue_id) {
                issue.state = normalized_state.clone();
            }
            self.operations
                .borrow_mut()
                .push(format!("add_project:{issue_id}:{normalized_state}"));
            Ok(())
        }

        fn set_project_field(
            &self,
            issue_ref: &str,
            assignment: &ProjectFieldAssignment,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            if let Some(issue) = self.issues.borrow_mut().get_mut(issue_ref) {
                issue.project_fields.insert(
                    assignment.name.clone(),
                    serde_json::Value::String(assignment.value.clone()),
                );
            }
            self.operations.borrow_mut().push(format!(
                "set_project_field:{issue_ref}:{}={}",
                assignment.name, assignment.value
            ));
            if self.fail_project_field_after_apply {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "GitHub GraphQL operation failed: HTTP 502 Bad Gateway".into(),
                    ),
                );
            }
            Ok(())
        }

        fn link_pull_request(
            &self,
            issue_ref: &str,
            pr_ref: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            if self.fail_link_pr {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "link failed".into(),
                    ),
                );
            }
            self.operations
                .borrow_mut()
                .push(format!("link_pr:{issue_ref}:{pr_ref}"));
            if self.confirm_link_pr {
                self.linked_pull_requests.borrow_mut().push(
                    jade_symphony::model::LinkedPullRequest {
                        number: pull_request_number_from_url(pr_ref),
                        url: Some(pr_ref.to_string()),
                        state: Some("OPEN".into()),
                        is_draft: Some(false),
                        ..Default::default()
                    },
                );
            }
            Ok(())
        }

        fn list_linked_pull_requests(
            &self,
            _issue_ref: &str,
        ) -> Result<
            Vec<jade_symphony::model::LinkedPullRequest>,
            jade_symphony::tracker::TrackerError,
        > {
            Ok(self.linked_pull_requests.borrow().clone())
        }

        fn close_issue(&self, issue_ref: &str) -> Result<(), jade_symphony::tracker::TrackerError> {
            self.operations
                .borrow_mut()
                .push(format!("close_issue:{issue_ref}"));
            if let Some(issue) = self.issues.borrow_mut().get_mut(issue_ref) {
                issue.project_fields.insert(
                    "GitHub Issue State".into(),
                    serde_json::Value::String("CLOSED".into()),
                );
            }
            if self.fail_close_after_apply {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "GitHub GraphQL operation failed: HTTP 502 Bad Gateway".into(),
                    ),
                );
            }
            Ok(())
        }
    }

    struct MergeRecoveryRunner {
        calls: RefCell<Vec<String>>,
    }

    impl MergeRecoveryRunner {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl HandoffCommandRunner for MergeRecoveryRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
            _cwd: &Path,
        ) -> Result<CommandOutput, jade_symphony::git_handoff::GitHandoffError> {
            self.calls
                .borrow_mut()
                .push(format!("{program} {}", args.join(" ")));
            if args.iter().any(|arg| arg == "merge") {
                return Ok(CommandOutput {
                    status: 1,
                    stdout: String::new(),
                    stderr: "GitHub returned HTTP 502 Bad Gateway after merge request".into(),
                });
            }
            if args.iter().any(|arg| arg == "view") {
                return Ok(CommandOutput {
                    status: 0,
                    stdout: serde_json::json!({
                        "number": 351,
                        "url": "https://github.com/Alive24/jade-symphony/pull/351",
                        "state": "MERGED",
                        "isDraft": false,
                        "mergeStateStatus": "CLEAN",
                        "reviewDecision": "APPROVED",
                        "baseRefName": "main",
                        "headRefName": "feature/issue-351",
                        "statusCheckRollup": []
                    })
                    .to_string(),
                    stderr: String::new(),
                });
            }
            Ok(CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn recoverable_lane_claim_write_recovers_after_transient_failure() {
        let config = test_config();
        let adapter = RecordingAdapter {
            fail_project_field_after_apply: true,
            ..RecordingAdapter::default()
        };
        let issue = tracker_issue_with_ref("#351", "Recover claim", "Todo");
        adapter
            .issues
            .borrow_mut()
            .insert(issue.identifier.clone(), issue.clone());
        let claim = LaneClaim::active(
            "#351",
            LaneClaimLane::Main,
            LaneClaimActor::Codex,
            LaneClaimSource::Loop,
            1_779_100_000_000,
        )
        .with_worker("Jade Symphony Agent");
        let claim_value = claim.render();

        write_lane_claim_field(&config, &adapter, &issue, WorkerLane::Main, &claim, true).unwrap();

        let updated = adapter.get_issue("#351").unwrap().unwrap();
        assert_eq!(
            project_text_field(&updated, "Main Agent").as_deref(),
            Some(claim_value.as_str())
        );
        assert_eq!(
            adapter.operations(),
            vec![format!("set_project_field:#351:Main Agent={claim_value}")]
        );
    }

    #[test]
    fn recoverable_timeline_comment_recovers_and_skips_duplicate_marker() {
        let adapter = RecordingAdapter {
            fail_comment_after_apply: true,
            ..RecordingAdapter::default()
        };
        let mut issue = tracker_issue_with_ref("#351", "Recover evidence", "Merging");
        issue.description = Some("## Issue body".into());
        adapter
            .issues
            .borrow_mut()
            .insert(issue.identifier.clone(), issue.clone());
        let key = recovery_key("merge-evidence", &issue.identifier, "run-1|pr-351");
        let workpad = "## Jade Symphony Merge Run\n\n- Result: `merged_or_done`";

        let first = add_timeline_comment_with_recovery(
            &adapter,
            &issue.identifier,
            Some(&issue),
            workpad,
            &key,
            "timeline_comment",
        )
        .unwrap();
        let updated = adapter.get_issue("#351").unwrap().unwrap();
        let second = add_timeline_comment_with_recovery(
            &adapter,
            &issue.identifier,
            Some(&updated),
            workpad,
            &key,
            "timeline_comment",
        )
        .unwrap();

        assert_eq!(first, TrackerMutationOutcome::Recovered);
        assert_eq!(second, TrackerMutationOutcome::AlreadyApplied);
        assert_eq!(adapter.operations(), vec!["comment:#351".to_string()]);
        assert!(updated
            .description
            .as_deref()
            .unwrap()
            .contains(&tracker_recovery_marker(&key)));
    }

    #[test]
    fn recoverable_state_write_recovers_after_transient_failure() {
        let adapter = RecordingAdapter {
            fail_state_after_apply: true,
            ..RecordingAdapter::default()
        };
        let issue = tracker_issue_with_ref("#351", "Recover state", "Merging");
        adapter
            .issues
            .borrow_mut()
            .insert(issue.identifier.clone(), issue.clone());

        let outcome = set_state_with_recovery(
            &adapter,
            &issue.identifier,
            Some(&issue),
            "done",
            "state_change",
        )
        .unwrap();

        assert_eq!(outcome, TrackerMutationOutcome::Recovered);
        assert_eq!(
            adapter
                .get_issue("#351")
                .unwrap()
                .unwrap()
                .normalized_state(),
            "done"
        );
    }

    #[test]
    fn recoverable_issue_close_recovers_after_transient_failure() {
        let adapter = RecordingAdapter {
            fail_close_after_apply: true,
            ..RecordingAdapter::default()
        };
        let mut issue = tracker_issue_with_ref("#351", "Recover close", "Done");
        issue.project_fields.insert(
            "GitHub Issue State".into(),
            serde_json::Value::String("OPEN".into()),
        );
        adapter
            .issues
            .borrow_mut()
            .insert(issue.identifier.clone(), issue.clone());

        let outcome = close_issue_with_recovery(&adapter, &issue.identifier, Some(&issue)).unwrap();

        assert_eq!(outcome, TrackerMutationOutcome::Recovered);
        assert!(issue_is_closed(
            &adapter.get_issue("#351").unwrap().unwrap()
        ));
    }

    #[test]
    fn recoverable_pr_merge_uses_readback_when_command_fails_after_merge() {
        let runner = MergeRecoveryRunner::new();

        let (output, outcome) = merge_pull_request_with_recovery(
            "https://github.com/Alive24/jade-symphony/pull/351",
            &runner,
            Path::new("."),
        )
        .unwrap();

        assert_eq!(outcome, TrackerMutationOutcome::Recovered);
        assert_eq!(output.status, 0);
        assert!(output.stdout.contains("readback shows PR merged"));
        let calls = runner.calls.borrow();
        assert!(calls.iter().any(|call| call.contains("pr merge")));
        assert!(calls.iter().any(|call| call.contains("pr view")));
    }

    #[test]
    fn doctor_hydrates_only_issue_states_that_need_rich_evidence() {
        let adapter = RecordingAdapter::default();
        let issues = vec![
            tracker_issue_with_ref("#1", "Backlog", "Backlog"),
            tracker_issue_with_ref("#2", "Done", "Done"),
            tracker_issue_with_ref("#3", "Agent Review", "Agent Review"),
            tracker_issue_with_ref("#4", "Todo", "Todo"),
            tracker_issue_with_ref("#5", "Need Human Input", "Need Human Input"),
            tracker_issue_with_ref("#6", "Rework", "Rework"),
        ];

        let hydrated = hydrate_issues_for_doctor(&adapter, issues).unwrap();

        assert_eq!(adapter.hydrated_issues.borrow().as_slice(), ["#3", "#4"]);
        assert_eq!(
            hydrated[2].description.as_deref(),
            Some("rich evidence for #3")
        );
        assert_eq!(
            hydrated[3].description.as_deref(),
            Some("rich evidence for #4")
        );
        assert_eq!(hydrated[4].description, None);
        assert_eq!(hydrated[5].description, None);
    }

    #[test]
    fn doctor_hydrates_active_native_topology_and_declared_subissues() {
        let adapter = RecordingAdapter::default();
        let mut parent = tracker_issue_with_ref("#243", "Parent", "Rework");
        parent.project_fields.insert(
            "GitHub Native Subissues".into(),
            serde_json::json!([
                {"identifier": "#272", "project_state": "Done"},
                {"identifier": "#273", "project_state": "Agent Review"}
            ]),
        );
        let done_subissue = tracker_issue_with_ref("#272", "Done subissue", "Done");
        let active_subissue = tracker_issue_with_ref("#273", "Active subissue", "Agent Review");
        let backlog_parent = {
            let mut issue = tracker_issue_with_ref("#300", "Backlog parent", "Backlog");
            issue.project_fields.insert(
                "GitHub Native Subissues".into(),
                serde_json::json!([{"identifier": "#301", "project_state": "Todo"}]),
            );
            issue
        };

        let _ = hydrate_issues_for_doctor(
            &adapter,
            vec![parent, done_subissue, active_subissue, backlog_parent],
        )
        .unwrap();

        assert_eq!(
            adapter.hydrated_issues.borrow().as_slice(),
            ["#243", "#272", "#273"]
        );
    }

    fn active_runtime_state(identifier: &str) -> RuntimeState {
        let mut state = RuntimeState::active(
            RuntimeIssueState {
                id: "ISSUE_29".into(),
                identifier: identifier.into(),
            },
            "dry-run",
        );
        state.updated_at_ms = Some(1_000);
        state
    }

    fn runtime_reconcile_test_config(root: &Path) -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nartifacts:\n  root: {:?}\n  namespace: Alive24/jade-symphony\nobservability:\n  logs_root: {:?}\n---\nPrompt",
                root.display().to_string(),
                root.join("logs").display().to_string()
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn main_tmux_session_record(
        issue_identifier: &str,
        status: SessionStatus,
    ) -> AgentSessionRecord {
        AgentSessionRecord {
            issue_id: Some("ISSUE_338".into()),
            issue_identifier: Some(issue_identifier.into()),
            issue_title: Some("Reconcile completed main tmux sessions after handoff".into()),
            lane: "main".into(),
            run_id: Some("20260520T0403Z-issue338-main-c91b".into()),
            thread: None,
            session_source: Some("loop".into()),
            claim_value: None,
            actor_role: Some("codex".into()),
            actor_label: Some("Codex manual main issue-338".into()),
            git_author: None,
            profile_id: None,
            instance_name: None,
            worktree: PathBuf::from("/tmp/issue-338"),
            branch: Some("feature/issue-338".into()),
            backend: "tmux".into(),
            session_name: "jade-main-338-attempt-1-reconcile".into(),
            pane_target: "jade-main-338-attempt-1-reconcile".into(),
            prompt_artifact_path: PathBuf::from("/tmp/prompt.md"),
            log_path: PathBuf::from("/tmp/session.log"),
            attach_command: "tmux attach-session -t jade-main-338-attempt-1-reconcile".into(),
            attempt: 1,
            status,
            started_at_ms: 1_000,
            updated_at_ms: 1_000,
        }
    }

    fn init_clean_git_workspace(path: &Path) {
        let output = ProcessCommand::new("git")
            .arg("init")
            .arg(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn clap_parser_preserves_default_plan_compatibility() {
        assert_eq!(
            parse(&[]),
            Command::Plan {
                workflow_path: PathBuf::from("WORKFLOW.md"),
                json: false,
            }
        );
        assert_eq!(
            parse(&["examples/dry-run-workflow.md"]),
            Command::Plan {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
                json: false,
            }
        );
    }

    #[test]
    fn clap_parser_keeps_operator_command_aliases() {
        assert!(
            Command::parse(vec!["status".into(), "examples/dry-run-workflow.md".into()]).is_err()
        );
        assert_eq!(
            parse(&["validate-workflow", "examples/dry-run-workflow.md"]),
            Command::Validate {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md")
            }
        );
        assert_eq!(
            parse(&["audit-project", "examples/dry-run-workflow.md"]),
            Command::Doctor {
                options: DoctorOptions {
                    workflow_path: Some(PathBuf::from("examples/dry-run-workflow.md")),
                    json: false,
                    strict: false,
                    display: DisplayMode::Plain,
                    interactive: false,
                    auto_fix: false,
                    write: false,
                    stale_after_ms: 10_800_000,
                    action: None,
                }
            }
        );
        assert_eq!(
            parse(&["profiles", "examples/dry-run-workflow.md"]),
            Command::Profiles {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md")
            }
        );
        assert_eq!(
            parse(&["debug", "examples/dry-run-workflow.md"]),
            Command::Debug {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md")
            }
        );
    }

    #[test]
    fn parses_inspect_state_filters() {
        assert_eq!(
            parse(&[
                "project",
                "inspect",
                "examples/github-project-workflow.md",
                "#284",
                "--lane",
                "main"
            ]),
            Command::ProjectInspect {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                issue_ref: "#284".into(),
                lane: Some(AgentSessionLaneArg::Main),
            }
        );
    }

    #[test]
    fn parses_project_state_read_surface() {
        assert_eq!(
            parse(&["project", "state", "examples/github-project-workflow.md"]),
            Command::ProjectState {
                options: ProjectStateOptions {
                    workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                    display: DisplayMode::Plain,
                }
            }
        );
    }

    #[test]
    fn parses_project_state_tui_display() {
        assert_eq!(
            parse(&[
                "project",
                "state",
                "examples/github-project-workflow.md",
                "--display",
                "tui"
            ]),
            Command::ProjectState {
                options: ProjectStateOptions {
                    workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                    display: DisplayMode::Tui,
                }
            }
        );
    }

    #[test]
    fn inspect_state_filter_matches_normalized_states() {
        let issues = vec![
            tracker_issue("Todo"),
            tracker_issue("In Progress"),
            tracker_issue("Agent Review"),
        ];

        let filtered = filter_issues_by_state(issues, &["in progress".into(), "todo".into()]);

        assert_eq!(
            filtered
                .into_iter()
                .map(|issue| issue.state)
                .collect::<Vec<_>>(),
            vec!["Todo", "In Progress"]
        );
    }

    #[test]
    fn inspect_state_filter_preserves_unfiltered_issue_list() {
        let issues = vec![tracker_issue("Todo"), tracker_issue("Done")];

        assert_eq!(filter_issues_by_state(issues.clone(), &[]), issues);
    }

    #[test]
    fn debug_helpers_summarize_sessions_and_health() {
        let clean = ProjectAuditReport {
            total_issues: 1,
            violations: Vec::new(),
            integration_gaps: Vec::new(),
            skill_readiness_summary: None,
        };
        assert_eq!(doctor_health_label(&clean), "clean");

        let warning_violation = ProjectAuditViolation {
            issue_ref: "#1".into(),
            title: "Needs owner".into(),
            state: "In Progress".into(),
            severity: AuditSeverity::Warning,
            code: "in_progress_missing_runtime_owner".into(),
            message: "missing owner".into(),
            suggestion: "inspect".into(),
        };
        let warning = ProjectAuditReport {
            total_issues: 1,
            violations: vec![warning_violation.clone()],
            integration_gaps: Vec::new(),
            skill_readiness_summary: None,
        };
        assert_eq!(doctor_health_label(&warning), "needs_attention");

        let blocked = ProjectAuditReport {
            total_issues: 1,
            violations: vec![ProjectAuditViolation {
                severity: AuditSeverity::Blocker,
                ..warning_violation
            }],
            integration_gaps: Vec::new(),
            skill_readiness_summary: None,
        };
        assert_eq!(doctor_health_label(&blocked), "blocked");

        let sessions = vec![
            SessionStatusSnapshot {
                session_id: "one".into(),
                lane: "main".into(),
                backend: "tmux".into(),
                run_id: None,
                status: "running".into(),
                evidence_source: "pane".into(),
                evidence: "active".into(),
                issue_identifier: Some("#1".into()),
                issue_title: Some("First".into()),
                attach_command: None,
                log_path: None,
                updated_at_ms: 1,
            },
            SessionStatusSnapshot {
                session_id: "two".into(),
                lane: "review".into(),
                backend: "tmux".into(),
                run_id: None,
                status: "running".into(),
                evidence_source: "pane".into(),
                evidence: "active".into(),
                issue_identifier: Some("#2".into()),
                issue_title: Some("Second".into()),
                attach_command: None,
                log_path: None,
                updated_at_ms: 2,
            },
        ];
        assert_eq!(session_status_summary(&sessions), "running:2");
    }

    #[test]
    fn render_state_summary_counts_states_in_stable_order() {
        let issues = vec![
            tracker_issue("Rework"),
            tracker_issue("Agent Review"),
            tracker_issue("Rework"),
            tracker_issue(""),
        ];

        assert_eq!(
            render_state_summary(&issues),
            "state_summary=(unknown):1, Agent Review:1, Rework:2"
        );
    }

    #[test]
    fn render_state_summary_handles_empty_issue_list() {
        assert_eq!(render_state_summary(&[]), "state_summary=(none)");
    }

    #[test]
    fn parses_status_json_flag() {
        assert_eq!(
            parse(&["status", "show", "examples/dry-run-workflow.md", "--json"]),
            Command::Plan {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
                json: true,
            }
        );
    }

    #[test]
    fn parses_skills_status_readiness_command() {
        assert_eq!(
            parse(&[
                "skills",
                "status",
                "workflows/jade-symphony.md",
                "--suite-path",
                "skills/jade-symphony/suite",
                "--session-skills",
                "jade-symphony-doctor,jade-symphony-manual-main",
                "--require-gemini",
                "--json",
            ]),
            Command::SkillsStatus {
                input: SkillStatusInput {
                    workflow_path: PathBuf::from("workflows/jade-symphony.md"),
                    suite_path: Some(PathBuf::from("skills/jade-symphony/suite")),
                    codex_dir: None,
                    gemini_dir: None,
                    require_gemini: true,
                    session_skills: vec!["jade-symphony-doctor,jade-symphony-manual-main".into()],
                    session_skills_file: None,
                },
                json: true,
            }
        );
    }

    #[test]
    fn parses_doctor_repair_human_review_command() {
        assert_eq!(
            parse(&[
                "doctor-repair-human-review",
                "examples/github-project-workflow.md",
                "--dry-run"
            ]),
            Command::DoctorRepairHumanReview {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                write: false
            }
        );
        assert_eq!(
            parse(&[
                "doctor-repair-human-review",
                "examples/github-project-workflow.md",
                "--write"
            ]),
            Command::DoctorRepairHumanReview {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                write: true
            }
        );
    }

    #[test]
    fn renders_plan_snapshot_as_json_when_requested() {
        let snapshot = jade_symphony::model::RuntimeSnapshot {
            event_log_path: Some("/tmp/jade-symphony.jsonl".into()),
            integration_gaps: vec!["gap".into()],
            ..Default::default()
        };

        let rendered = render_plan_snapshot(&snapshot, true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(
            value
                .get("event_log_path")
                .and_then(serde_json::Value::as_str),
            Some("/tmp/jade-symphony.jsonl")
        );
        assert_eq!(
            value
                .pointer("/integration_gaps/0")
                .and_then(serde_json::Value::as_str),
            Some("gap")
        );
    }

    #[test]
    fn parses_doctor_json_and_strict_flags() {
        assert_eq!(
            parse(&[
                "doctor",
                "examples/github-project-workflow.md",
                "--json",
                "--strict"
            ]),
            Command::Doctor {
                options: DoctorOptions {
                    workflow_path: Some(PathBuf::from("examples/github-project-workflow.md")),
                    json: true,
                    strict: true,
                    display: DisplayMode::Plain,
                    interactive: false,
                    auto_fix: false,
                    write: false,
                    stale_after_ms: 10_800_000,
                    action: None,
                }
            }
        );
    }

    #[test]
    fn parses_short_doctor_commands() {
        assert_eq!(
            parse(&["doctor", "--interactive"]),
            Command::Doctor {
                options: DoctorOptions {
                    workflow_path: None,
                    json: false,
                    strict: false,
                    display: DisplayMode::Plain,
                    interactive: true,
                    auto_fix: false,
                    write: false,
                    stale_after_ms: 10_800_000,
                    action: None,
                }
            }
        );
        assert_eq!(
            parse(&["doctor", "--auto-fix", "--dry-run"]),
            Command::Doctor {
                options: DoctorOptions {
                    workflow_path: None,
                    json: false,
                    strict: false,
                    display: DisplayMode::Plain,
                    interactive: false,
                    auto_fix: true,
                    write: false,
                    stale_after_ms: 10_800_000,
                    action: None,
                }
            }
        );
        assert_eq!(
            parse(&["doctor", "repair", "194"]),
            Command::Doctor {
                options: DoctorOptions {
                    workflow_path: None,
                    json: false,
                    strict: false,
                    display: DisplayMode::Plain,
                    interactive: false,
                    auto_fix: false,
                    write: false,
                    stale_after_ms: 10_800_000,
                    action: Some(DoctorAction::Repair(DoctorRepairIssueOptions {
                        issue_ref: "194".into(),
                        write: false,
                        move_need_human_input: false,
                        mark_pr_ready: false,
                        confirm_handoff_ready: false,
                    })),
                }
            }
        );
    }

    #[test]
    fn parses_status_api_command() {
        assert_eq!(
            parse(&[
                "status",
                "serve",
                "examples/dry-run-workflow.md",
                "--bind",
                "127.0.0.1:0",
                "--once"
            ]),
            Command::StatusApi {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
                bind: "127.0.0.1:0".parse().unwrap(),
                once: true,
            }
        );
    }

    #[test]
    fn parses_agent_session_commands() {
        assert_eq!(
            parse(&[
                "session",
                "start",
                "workflows/jade-symphony.md",
                "#220",
                "--lane",
                "review",
                "--run",
                "20260517T1404Z-issue220-review-manual",
                "--write"
            ]),
            Command::SessionStart {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
                issue_ref: "#220".into(),
                lane: AgentSessionLaneArg::Review,
                run_id: "20260517T1404Z-issue220-review-manual".into(),
                write: true,
            }
        );
        assert_eq!(
            parse(&["session", "list", "workflows/jade-symphony.md"]),
            Command::SessionList {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
            }
        );
        assert_eq!(
            parse(&[
                "session",
                "attach",
                "workflows/jade-symphony.md",
                "jade-review-220"
            ]),
            Command::SessionAttach {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
                session: "jade-review-220".into(),
                exec: false,
            }
        );
        assert!(Command::parse(vec!["agent-session".into(), "list".into()]).is_err());
        assert!(Command::parse(vec!["review-session".into(), "WORKFLOW.md".into()]).is_err());
        assert!(Command::parse(vec!["merge-session".into(), "WORKFLOW.md".into()]).is_err());
    }

    #[test]
    fn parses_lane_claim_command_groups() {
        assert_eq!(
            parse(&[
                "main",
                "claim",
                "workflows/jade-symphony.md",
                "#265",
                "--worker",
                "codex-manual-main",
                "--source",
                "manual",
                "--write"
            ]),
            Command::LaneClaim {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
                issue_ref: "#265".into(),
                lane: AgentSessionLaneArg::Main,
                worker: "codex-manual-main".into(),
                source: CliLaneClaimSource::Manual,
                write: true,
            }
        );
        assert_eq!(
            parse(&[
                "review",
                "claim",
                "workflows/jade-symphony.md",
                "#265",
                "--worker",
                "gemini-manual-review"
            ]),
            Command::LaneClaim {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
                issue_ref: "#265".into(),
                lane: AgentSessionLaneArg::Review,
                worker: "gemini-manual-review".into(),
                source: CliLaneClaimSource::Manual,
                write: false,
            }
        );
    }

    #[test]
    fn parses_unified_session_commands() {
        assert_eq!(
            parse(&[
                "session",
                "start",
                "workflows/jade-symphony.md",
                "#265",
                "--lane",
                "main",
                "--run",
                "20260517T0909Z-issue265-main-manual",
                "--write"
            ]),
            Command::SessionStart {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
                issue_ref: "#265".into(),
                lane: AgentSessionLaneArg::Main,
                run_id: "20260517T0909Z-issue265-main-manual".into(),
                write: true,
            }
        );
        assert_eq!(
            parse(&["session", "list", "workflows/jade-symphony.md"]),
            Command::SessionList {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
            }
        );
    }

    #[test]
    fn review_session_uses_gemini_command_when_no_tmux_override_exists() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: tmux\ntmux:\n  agent_command: codex\nreview_lane:\n  backend: gemini-cli\n  gemini_command: /opt/homebrew/bin/gemini\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(
            tmux_agent_command_for_lane(&config, AgentSessionLaneArg::Main).unwrap(),
            "codex"
        );
        assert_eq!(
            tmux_agent_command_for_lane(&config, AgentSessionLaneArg::Review).unwrap(),
            "/opt/homebrew/bin/gemini"
        );
        assert_eq!(
            tmux_agent_command_for_lane(&config, AgentSessionLaneArg::Merge).unwrap(),
            "codex"
        );
    }

    #[test]
    fn review_session_prefers_tmux_review_command_override() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: tmux\ntmux:\n  agent_command: codex\n  review_agent_command: custom-gemini --model pro\nreview_lane:\n  backend: gemini-cli\n  gemini_command: /opt/homebrew/bin/gemini\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(
            tmux_agent_command_for_lane(&config, AgentSessionLaneArg::Review).unwrap(),
            "custom-gemini --model pro"
        );
    }

    #[test]
    fn main_session_defaults_to_codex_app_server_command() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: codex\ncodex:\n  command: /opt/homebrew/bin/codex app-server\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        let spec = agent_session_backend_spec(&config, AgentSessionLaneArg::Main).unwrap();

        assert_eq!(spec.backend, "codex");
        assert_eq!(spec.command, "/opt/homebrew/bin/codex app-server");
    }

    #[test]
    fn main_session_keeps_tmux_as_explicit_fallback() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: tmux\ntmux:\n  agent_command: codex\n  main_agent_command: codex --profile main\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        let spec = agent_session_backend_spec(&config, AgentSessionLaneArg::Main).unwrap();

        assert_eq!(spec.backend, "tmux");
        assert_eq!(spec.command, "codex --profile main");
    }

    #[test]
    fn merge_session_defaults_to_codex_app_server_command() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\ncodex:\n  command: /opt/homebrew/bin/codex app-server\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        let spec = agent_session_backend_spec(&config, AgentSessionLaneArg::Merge).unwrap();

        assert_eq!(spec.backend, "codex");
        assert_eq!(spec.command, "/opt/homebrew/bin/codex app-server");
    }

    #[test]
    fn merge_session_keeps_tmux_as_explicit_fallback() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmerge_lane:\n  agent_backend: tmux\ntmux:\n  agent_command: codex\n  merge_agent_command: codex --profile merge\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        let spec = agent_session_backend_spec(&config, AgentSessionLaneArg::Merge).unwrap();

        assert_eq!(spec.backend, "tmux");
        assert_eq!(spec.command, "codex --profile merge");
    }

    #[test]
    fn clean_merge_tick_does_not_require_merge_agent_backend() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_path = temp.path().join("WORKFLOW.md");
        std::fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: memory\nmerge_lane:\n  agent_backend: definitely-not-a-session-backend\n---\nPrompt",
        )
        .unwrap();

        let outcome = merge_once_tick(workflow_path, false, false).unwrap();

        assert_eq!(outcome, MergeOnceOutcome::NoMergingIssue);
    }

    #[test]
    fn successful_merge_agent_repair_records_merging_retry_rationale() {
        let config = test_config();
        let issue = tracker_issue_with_ref("#390", "Route DIRTY repair", "Merging");
        let runner = MergeRecoveryRunner::new();

        let outcome = finish_merge_agent_repaired_branch(
            &config,
            &issue,
            "merge_agent",
            "src/main.rs conflicted",
            "resolved conflict by preserving approved behavior",
            "approved implementation intent preserved",
            vec![
                "git diff --name-only --diff-filter=U".into(),
                "git diff --check".into(),
                "git status --porcelain".into(),
            ],
            "https://github.com/Alive24/jade-symphony/pull/390",
            "feature/issue-390",
            &runner,
            Path::new("."),
            CommandOutput {
                status: 0,
                stdout: "MERGE_AGENT_DECISION: repaired".into(),
                stderr: String::new(),
            },
            "codex".into(),
            Some("session-390".into()),
        )
        .unwrap();

        assert!(outcome.repaired);
        assert_eq!(outcome.evidence.method, "merge_agent");
        assert!(outcome
            .evidence
            .next_state_rationale
            .contains("stays in `Merging`"));
        assert!(runner
            .calls
            .borrow()
            .iter()
            .any(|call| call == "git push origin feature/issue-390"));
    }

    #[test]
    fn merge_agent_semantic_uncertainty_marker_requires_human_input() {
        let text = "\
RESOLUTION_SUMMARY: conflict needs product choice
SEMANTIC_SAFETY: cannot prove reviewed intent
MERGE_AGENT_DECISION: needs_human_input";

        assert!(merge_agent_requests_human_input(text));
        assert!(!merge_agent_reports_repaired(text));
    }

    #[test]
    fn dogfood_smoke_is_not_a_cli_entrypoint() {
        let help = help_text(&["--help"]);
        assert!(!help.contains("dogfood-smoke"));

        let error = Command::parse(vec![
            "dogfood-smoke".into(),
            "examples/github-project-workflow.md".into(),
            "--dry-run".into(),
        ])
        .unwrap_err();

        assert!(error.contains("unexpected argument 'examples/github-project-workflow.md'"));
    }

    #[test]
    fn parses_cleanup_plan_command() {
        assert_eq!(
            parse(&["clean", "plan", "examples/github-project-workflow.md"]),
            Command::CleanPlan {
                workflow_path: PathBuf::from("examples/github-project-workflow.md")
            }
        );
        assert_eq!(
            parse(&["clean", "audit", "examples/github-project-workflow.md"]),
            Command::CleanAudit {
                workflow_path: PathBuf::from("examples/github-project-workflow.md")
            }
        );
    }

    #[test]
    fn parses_cleanup_workspaces_command() {
        assert!(Command::parse(vec![
            "cleanup-workspaces".into(),
            "examples/github-project-workflow.md".into(),
            "--write".into()
        ])
        .is_err());
        assert!(Command::parse(vec![
            "workspace-cleanup".into(),
            "examples/github-project-workflow.md".into()
        ])
        .is_err());
    }

    #[test]
    fn parses_workspace_discovery_commands() {
        assert_eq!(
            parse(&["workspace", "list", "workflows/jade-symphony.md"]),
            Command::WorkspaceList {
                workflow_path: PathBuf::from("workflows/jade-symphony.md")
            }
        );
        assert_eq!(
            parse(&["workspace", "show", "workflows/jade-symphony.md", "#253"]),
            Command::WorkspaceShow {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
                issue_ref: "#253".into(),
            }
        );
        assert_eq!(
            parse(&[
                "workspace",
                "adopt",
                "workflows/jade-symphony.md",
                "#253",
                "/tmp/issue-253",
                "--write"
            ]),
            Command::WorkspaceAdopt {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
                issue_ref: "#253".into(),
                path: PathBuf::from("/tmp/issue-253"),
                write: true,
            }
        );
        assert_eq!(
            parse(&[
                "workspace",
                "ensure",
                "workflows/jade-symphony.md",
                "#253",
                "--pr",
                "254",
                "--branch",
                "feature/issue-253-worktree-discovery",
                "--write"
            ]),
            Command::WorkspaceEnsure {
                workflow_path: PathBuf::from("workflows/jade-symphony.md"),
                issue_ref: "#253".into(),
                pr_ref: Some("254".into()),
                branch: Some("feature/issue-253-worktree-discovery".into()),
                write: true,
            }
        );
    }

    #[test]
    fn workspace_ensure_creates_only_under_configured_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        ProcessCommand::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .args(["checkout", "-q", "-B", "main"])
            .current_dir(&repo)
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&repo)
            .status()
            .unwrap();
        std::fs::write(repo.join("README.md"), "repo\n").unwrap();
        ProcessCommand::new("git")
            .args(["add", "README.md"])
            .current_dir(&repo)
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .args(["commit", "-qm", "init"])
            .current_dir(&repo)
            .status()
            .unwrap();

        let workspace_root = temp.path().join("workspaces");
        let mut issue = tracker_issue("Agent Review");
        issue.identifier = "#271".into();
        issue.title = "Add safe workspace ensure for Review and Merge inspection".into();
        issue.branch_name = None;
        let plan = plan_issue_handoff_for_profile(&workspace_root, &issue, "main", None).unwrap();

        validate_workspace_path_under_root(&workspace_root, &plan.workspace_path).unwrap();
        ensure_inspection_worktree(&repo, &plan.workspace_path, &plan.branch_name, None).unwrap();

        assert!(plan.workspace_path.starts_with(&workspace_root));
        assert!(plan.workspace_path.is_dir());
        assert_eq!(
            current_git_branch(&plan.workspace_path).unwrap().as_deref(),
            Some(plan.branch_name.as_str())
        );
    }

    #[test]
    fn workspace_cleanup_plan_marks_terminal_existing_workspace_eligible() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.workspace.root = temp.path().join("workspaces");
        let issue = tracker_issue("Done");
        let handoff =
            plan_issue_handoff_for_profile(&config.workspace.root, &issue, "main", None).unwrap();
        std::fs::create_dir_all(&handoff.workspace_path).unwrap();

        let entries = workspace_cleanup_plan(&config, &[issue]).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].issue_ref, "#29");
        assert_eq!(entries[0].workspace_key, handoff.workspace_key);
        assert_eq!(entries[0].action, WorkspaceCleanupAction::Eligible);
    }

    #[test]
    fn workspace_cleanup_plan_skips_non_terminal_and_missing_workspaces() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.workspace.root = temp.path().join("workspaces");
        let mut active = tracker_issue("In Progress");
        active.identifier = "#30".into();
        active.title = "Active workspace".into();
        active.branch_name = None;
        let missing_terminal = tracker_issue("Done");

        let entries = workspace_cleanup_plan(&config, &[active, missing_terminal]).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].action,
            WorkspaceCleanupAction::Skipped {
                reason: "non_terminal_state".into()
            }
        );
        assert_eq!(
            entries[1].action,
            WorkspaceCleanupAction::Skipped {
                reason: "workspace_missing".into()
            }
        );
    }

    #[test]
    fn all_mapped_tracker_states_includes_merging_for_doctor() {
        let config = test_config();
        let states = all_mapped_tracker_states(&config);

        assert!(states.contains(&"Merging".to_string()));
        assert!(states.contains(&"Rework".to_string()));
        assert!(states.contains(&"Done".to_string()));
    }

    #[test]
    fn controlled_smoke_issue_requires_marker_label_or_title() {
        let mut issue = tracker_issue("Todo");
        assert!(!is_controlled_dogfood_smoke_issue(&issue));

        issue.labels = vec!["dogfood-smoke".into()];
        assert!(is_controlled_dogfood_smoke_issue(&issue));

        issue.labels.clear();
        issue.title = "[dogfood-smoke] controlled run".into();
        assert!(is_controlled_dogfood_smoke_issue(&issue));
    }

    #[test]
    fn main_app_server_smoke_gate_is_ready_for_codex_app_server() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: codex\ncodex:\n  command: /opt/homebrew/bin/codex app-server\n  approval_policy: never\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        let gate = main_app_server_smoke_gate(&config);

        assert_eq!(gate.backend, "codex");
        assert_eq!(gate.backend_source, "codex-app-server");
        assert_eq!(gate.command, "/opt/homebrew/bin/codex app-server");
        assert_eq!(gate.approval_policy, "never");
        assert!(gate.app_server_live_smoke_ready);
    }

    #[test]
    fn main_app_server_smoke_gate_rejects_non_app_server_codex() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: codex\ncodex:\n  command: codex exec\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        let gate = main_app_server_smoke_gate(&config);

        assert_eq!(gate.backend, "codex");
        assert_eq!(gate.backend_source, "codex-subprocess");
        assert!(!gate.app_server_live_smoke_ready);
        assert!(gate
            .app_server_live_smoke_reason
            .contains("does not select the app-server"));
    }

    #[test]
    fn dogfood_smoke_classifies_accepted_adapter_gaps_as_warnings() {
        let gaps = vec![
            "GitHub Project v2 PR linking uses an issue comment/autolink strategy; linked PR discovery currently reads closing PR references.".into(),
            "GitHub Project v2 live write methods use `gh api graphql`; keep using `--write` for mutating CLI commands.".into(),
            "Linear pull request linking currently records a tracker comment rather than a first-class Linear attachment.".into(),
        ];

        let report = classify_dogfood_integration_gaps(&gaps);

        assert!(report.blocking.is_empty());
        assert_eq!(report.warnings, gaps);
    }

    #[test]
    fn dogfood_smoke_keeps_unknown_or_runtime_gaps_blocking() {
        let gaps = vec![
            "GitHub Project v2 is using fixture issues because tracker.fixture_path is set.".into(),
            "unexpected live tracker blocker".into(),
        ];

        let report = classify_dogfood_integration_gaps(&gaps);

        assert_eq!(report.blocking, gaps);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn clap_parser_treats_help_flags_as_successful_help() {
        assert!(help_text(&["--help"]).contains("Usage: jade-symphony"));
        assert!(help_text(&["-h"]).contains("Usage: jade-symphony"));
    }

    #[test]
    fn clap_parser_preserves_subcommand_specific_help() {
        let link_pr = help_text(&["project", "link-pr", "--help"]);
        assert!(link_pr.contains("Usage: jade-symphony project link-pr"));
        assert!(link_pr.contains("<path-to-WORKFLOW.md>"));
        assert!(link_pr.contains("<ISSUE_REF>"));
        assert!(link_pr.contains("<PR_REF>"));

        let workpad = help_text(&["project", "workpad", "--help"]);
        assert!(workpad.contains("Usage: jade-symphony project workpad"));
        assert!(workpad.contains("<MARKDOWN_PATH>"));

        let set_state = help_text(&["project", "set-state", "--help"]);
        assert!(set_state.contains("Usage: jade-symphony project set-state"));
        assert!(set_state.contains("<STATE>"));

        let forge_promote = help_text(&["forge", "promote", "--help"]);
        assert!(forge_promote.contains("Usage: jade-symphony forge promote"));
        assert!(forge_promote.contains("--operator-confirmation"));
        assert!(forge_promote.contains("--readback-summary"));
    }

    #[test]
    fn workspace_help_explains_discovery_and_adoption_boundaries() {
        let workspace = help_text(&["workspace", "--help"]);
        assert!(workspace.contains("Discover and record per-issue git worktrees"));
        assert!(workspace.contains("safe local-worktree coordination surface"));
        assert!(workspace.contains("never runs `gh pr checkout`"));

        let list = help_text(&["workspace", "list", "--help"]);
        assert!(list.contains("read-only Project-wide inventory"));
        assert!(list.contains("orphan-looking worktrees"));

        let show = help_text(&["workspace", "show", "--help"]);
        assert!(show.contains("read-only preflight for Review and Merge agents"));
        assert!(show.contains("Multiple strong candidates require operator choice"));

        let adopt = help_text(&["workspace", "adopt", "--help"]);
        assert!(adopt.contains("operator-selected existing worktree"));
        assert!(adopt.contains("It does not create a worktree"));
        assert!(adopt.contains("--write"));

        let ensure = help_text(&["workspace", "ensure", "--help"]);
        assert!(ensure.contains("reuse"));
        assert!(ensure.contains("workflow-configured workspace root"));
        assert!(ensure.contains("never runs `gh pr checkout`"));
        assert!(ensure.contains("Workspace Evidence"));
    }

    #[test]
    fn clap_parser_preserves_write_intent_for_mutating_commands() {
        assert_eq!(
            parse(&[
                "project",
                "set-state",
                "examples/github-project-workflow.md",
                "#4",
                "agent_review",
                "--write"
            ]),
            Command::SetState {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                issue_ref: "#4".into(),
                state: "agent_review".into(),
                write: true
            }
        );
    }

    #[test]
    fn clap_parser_preserves_review_outcome_mapping() {
        assert_eq!(
            parse(&[
                "review",
                "fake",
                "examples/github-project-workflow.md",
                "#4",
                "--outcome",
                "confirmed",
                "--write"
            ]),
            Command::ReviewFake {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                issue_ref: "#4".into(),
                outcome: FakeReviewOutcome::ConfirmedFinding,
                write: true
            }
        );
    }

    #[test]
    fn manual_lane_claim_with_display_worker_round_trips_to_session_start_validation() {
        let mut issue = tracker_issue_with_ref("#297", "Support quoted worker labels", "Todo");
        let claim = lane_claim_for_manual_worker(
            &issue,
            AgentSessionLaneArg::Main,
            LaneClaimActor::Codex,
            LaneClaimSource::Manual,
            "Codex Manual Main",
            None,
        )
        .unwrap();
        let claim_value = render_parseable_lane_claim(&claim).unwrap();

        assert!(claim_value.contains("worker=\"Codex Manual Main\""));
        issue
            .project_fields
            .insert("Main Agent".into(), serde_json::Value::String(claim_value));

        let parsed =
            matching_lane_claim_for_session(&issue, AgentSessionLaneArg::Main, &claim.run).unwrap();

        assert_eq!(parsed.worker.as_deref(), Some("Codex Manual Main"));
        assert_eq!(parsed, claim);
    }

    #[test]
    fn parses_project_issue_read_surface() {
        assert_eq!(
            parse(&[
                "project",
                "issue",
                "examples/github-project-workflow.md",
                "#235",
                "--json"
            ]),
            Command::ProjectIssue {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                issue_ref: "#235".into(),
                json: true
            }
        );
    }

    #[test]
    fn parses_project_timeline_comment_write_surface() {
        assert_eq!(
            parse(&[
                "project",
                "timeline-comment",
                "examples/github-project-workflow.md",
                "#235",
                "/tmp/human-review-note.md",
                "--write"
            ]),
            Command::TimelineComment {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                issue_ref: "#235".into(),
                markdown_path: PathBuf::from("/tmp/human-review-note.md"),
                write: true,
            }
        );
    }

    #[test]
    fn parses_manual_review_authority_commands() {
        assert_eq!(
            parse(&[
                "review",
                "claim",
                "examples/github-project-workflow.md",
                "#235",
                "--worker",
                "Gemini A",
                "--write"
            ]),
            Command::LaneClaim {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                issue_ref: "#235".into(),
                lane: AgentSessionLaneArg::Review,
                worker: "Gemini A".into(),
                source: CliLaneClaimSource::Manual,
                write: true
            }
        );

        assert!(Command::parse(vec![
            "review-clear-claim".into(),
            "examples/github-project-workflow.md".into(),
            "#235".into(),
            "--write".into()
        ])
        .is_err());
    }

    #[test]
    fn review_group_help_hides_legacy_flat_review_commands() {
        let help = help_text(&["--help"]);

        assert!(help.contains("review"));
        assert!(!help.contains("review-claim"));
        assert!(!help.contains("review-pass"));
        assert!(!help.contains("review-reject"));
        assert!(!help.contains("review-clear-claim"));
    }

    #[test]
    fn parses_grouped_review_commands() {
        let command = Command::parse(vec![
            "review".into(),
            "loop".into(),
            "examples/review-fixture-workflow.md".into(),
            "--once".into(),
            "--fake-outcome".into(),
            "confirmed".into(),
        ])
        .unwrap();

        let Command::ReviewLoop { options } = command else {
            panic!("expected grouped review loop command");
        };
        assert_eq!(
            options.workflow_path,
            PathBuf::from("examples/review-fixture-workflow.md")
        );
        assert!(options.once);
        assert_eq!(
            options.fake_outcome,
            Some(FakeReviewOutcome::ConfirmedFinding)
        );
    }

    #[test]
    fn automatic_review_prompt_forbids_project_mutations() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\n---\nReview {{ issue.identifier }}",
        )
        .unwrap();
        let prompt = render_automatic_review_prompt(
            &workflow,
            &review_issue_with_ref("#282", "Headless review"),
        )
        .unwrap();

        assert!(prompt.contains("Review #282"));
        assert!(prompt.contains("Automatic Headless Review Boundary"));
        assert!(prompt.contains("Do not run mutating Jade Symphony or GitHub commands"));
        assert!(prompt.contains("`review claim`, `review pass`"));
        assert!(prompt.contains("`gh issue edit`, `gh issue comment`"));
        assert!(prompt.contains("Return review evidence in stdout only"));
        assert!(prompt.contains("Review Result: PASS"));
        assert!(prompt.contains("Do not use those bracketed finding tags for positive"));
        assert!(prompt.contains("Leave routing and evidence"));
    }

    #[test]
    fn manual_review_pass_workpad_records_doctor_evidence_marker() {
        let issue = tracker_issue_with_review_claim();
        let claim = project_text_field(&issue, "Review Agent").unwrap();
        let terminal = format!(
            "{} result=passed",
            LaneClaim::parse(&claim)
                .unwrap()
                .with_state(LaneClaimState::Done)
                .render()
        );
        let workpad = render_manual_review_workpad(
            &issue,
            "passed",
            "human_review",
            "Gemini: pass",
            true,
            &claim,
            &terminal,
        );

        assert!(workpad.contains("Reviewer backend: manual-operator"));
        assert!(workpad.contains("Review pass evidence: `recorded`"));
        assert!(workpad.contains("main implementation agent must not"));
        assert!(workpad.contains("Terminal Review Agent claim"));
    }

    #[test]
    fn manual_review_reject_workpad_does_not_record_pass_marker() {
        let issue = tracker_issue_with_review_claim();
        let claim = project_text_field(&issue, "Review Agent").unwrap();
        let terminal = format!(
            "{} result=inconclusive",
            LaneClaim::parse(&claim)
                .unwrap()
                .with_state(LaneClaimState::Failed)
                .render()
        );
        let workpad = render_manual_review_workpad(
            &issue,
            "not passed",
            "agent_review",
            "Gemini: inconclusive",
            false,
            &claim,
            &terminal,
        );

        assert!(!workpad.contains("Review pass evidence: `recorded`"));
        assert!(workpad.contains("must not move to Human Review"));
    }

    #[test]
    fn manual_review_claim_validation_requires_exact_evidence_claim() {
        let issue = tracker_issue_with_review_claim();
        let claim = project_text_field(&issue, "Review Agent").unwrap();

        assert!(validate_active_manual_review_claim(&issue, &format!("claim: {claim}")).is_ok());
        let error = validate_active_manual_review_claim(&issue, "claim: Manual Gemini A")
            .unwrap_err()
            .to_string();
        assert!(error.contains("exact current Review Agent claim"));
    }

    #[test]
    fn manual_review_pass_allows_terminal_passed_claim_repair() {
        let mut issue = tracker_issue_with_review_claim();
        let claim = project_text_field(&issue, "Review Agent").unwrap();
        let terminal = terminal_review_claim_value(
            &LaneClaim::parse(&claim).unwrap(),
            LaneClaimState::Done,
            "passed",
        );
        issue.project_fields.insert(
            "Review Agent".into(),
            serde_json::Value::String(terminal.clone()),
        );

        let (current, parsed) =
            validate_manual_review_pass_claim(&issue, &format!("claim: {terminal}")).unwrap();

        assert_eq!(current, terminal);
        assert_eq!(parsed.state, LaneClaimState::Done);
    }

    #[test]
    fn manual_review_reject_still_requires_active_claim() {
        let mut issue = tracker_issue_with_review_claim();
        let claim = project_text_field(&issue, "Review Agent").unwrap();
        let terminal = terminal_review_claim_value(
            &LaneClaim::parse(&claim).unwrap(),
            LaneClaimState::Done,
            "passed",
        );
        issue.project_fields.insert(
            "Review Agent".into(),
            serde_json::Value::String(terminal.clone()),
        );

        let error = validate_active_manual_review_claim(&issue, &format!("claim: {terminal}"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("must be active before routing"));
    }

    #[test]
    fn terminal_review_claim_records_result_without_losing_structured_claim() {
        let issue = tracker_issue_with_review_claim();
        let claim = LaneClaim::parse(&project_text_field(&issue, "Review Agent").unwrap()).unwrap();

        let value = terminal_review_claim_value(&claim, LaneClaimState::Done, "passed");

        assert!(value.contains("state=done"));
        assert!(value.contains("result=passed"));
        assert_eq!(
            LaneClaim::parse(&value)
                .unwrap()
                .with_state(LaneClaimState::Active),
            claim
        );
    }

    #[test]
    fn parses_review_freshness_command() {
        let command = Command::parse(vec![
            "review".into(),
            "freshness".into(),
            "--issue".into(),
            "#33".into(),
            "--prior-head".into(),
            "old-head".into(),
            "--current-head".into(),
            "new-head".into(),
            "--prior-base".into(),
            "old-base".into(),
            "--current-base".into(),
            "new-base".into(),
            "--changed-file".into(),
            "docs/dogfood-readiness.md".into(),
            "--stale-reason".into(),
            "merge-conflict".into(),
            "--rework-class".into(),
            "mechanical-conflict-resolution".into(),
            "--patch-summary".into(),
            "Resolved conflict without semantic changes.".into(),
        ])
        .unwrap();

        let Command::ReviewFreshness { input } = command else {
            panic!("expected review-freshness command");
        };

        assert_eq!(input.issue_ref, "#33");
        assert_eq!(input.changed_files, vec!["docs/dogfood-readiness.md"]);
        assert_eq!(input.stale_reason, ReviewStaleReason::MergeConflict);
        assert_eq!(
            input.rework_class,
            ReviewReworkClass::MechanicalConflictResolution
        );
        assert!(input.patch_summary.unwrap().contains("Resolved conflict"));
    }

    #[test]
    fn parses_review_loop_flags() {
        let command = Command::parse(vec![
            "review".into(),
            "loop".into(),
            "examples/review-fixture-workflow.md".into(),
            "--max-iterations".into(),
            "2".into(),
            "--fake-outcome".into(),
            "confirmed".into(),
            "--max-concurrent".into(),
            "2".into(),
            "--write".into(),
        ])
        .unwrap();

        let Command::ReviewLoop { options } = command else {
            panic!("expected review loop command");
        };

        assert_eq!(
            options.workflow_path,
            PathBuf::from("examples/review-fixture-workflow.md")
        );
        assert_eq!(options.max_iterations, Some(2));
        assert_eq!(
            options.fake_outcome,
            Some(FakeReviewOutcome::ConfirmedFinding)
        );
        assert_eq!(options.max_concurrent, Some(2));
        assert!(options.write);
    }

    #[test]
    fn review_loop_once_overrides_max_iterations() {
        let command = Command::parse(vec![
            "review".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "4".into(),
            "--once".into(),
        ])
        .unwrap();

        let Command::ReviewLoop { options } = command else {
            panic!("expected review loop command");
        };

        assert_eq!(options.iteration_limit(), Some(1));
    }

    #[test]
    fn parses_review_status_flags() {
        let command = Command::parse(vec![
            "review".into(),
            "status".into(),
            "examples/review-fixture-workflow.md".into(),
            "--issue".into(),
            "#313".into(),
            "--recent".into(),
            "3".into(),
            "--verbose".into(),
            "--json".into(),
        ])
        .unwrap();

        let Command::ReviewStatus { options } = command else {
            panic!("expected review status command");
        };

        assert_eq!(
            options.workflow_path,
            PathBuf::from("examples/review-fixture-workflow.md")
        );
        assert_eq!(options.issue_filter.as_deref(), Some("#313"));
        assert_eq!(options.recent_limit, 3);
        assert!(options.verbose);
        assert!(options.json);
    }

    #[test]
    fn parses_merge_loop_flags() {
        let command = Command::parse(vec![
            "merge".into(),
            "loop".into(),
            "examples/github-project-workflow.md".into(),
            "--max-iterations".into(),
            "3".into(),
            "--max-concurrent".into(),
            "2".into(),
            "--write".into(),
        ])
        .unwrap();

        let Command::MergeLoop { options } = command else {
            panic!("expected merge loop command");
        };

        assert_eq!(
            options.workflow_path,
            PathBuf::from("examples/github-project-workflow.md")
        );
        assert_eq!(options.max_iterations, Some(3));
        assert_eq!(options.max_concurrent, Some(2));
        assert_eq!(options.worker_limit(&test_config()), 2);
        assert!(options.write);
        assert!(options.recover);
    }

    #[test]
    fn merge_loop_no_recover_disables_write_default() {
        let command = Command::parse(vec![
            "merge".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "1".into(),
            "--write".into(),
            "--no-recover".into(),
        ])
        .unwrap();

        let Command::MergeLoop { options } = command else {
            panic!("expected merge loop command");
        };

        assert!(options.write);
        assert!(!options.recover);
    }

    #[test]
    fn merge_loop_once_overrides_max_iterations() {
        let command = Command::parse(vec![
            "merge".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "4".into(),
            "--once".into(),
        ])
        .unwrap();

        let Command::MergeLoop { options } = command else {
            panic!("expected merge loop command");
        };

        assert_eq!(options.iteration_limit(), Some(1));
    }

    #[test]
    fn parses_unbounded_merge_loop_without_max_iterations() {
        let command =
            Command::parse(vec!["merge".into(), "loop".into(), "WORKFLOW.md".into()]).unwrap();

        let Command::MergeLoop { options } = command else {
            panic!("expected merge loop command");
        };

        assert_eq!(options.iteration_limit(), None);
    }

    #[test]
    fn rejects_zero_merge_loop_iterations() {
        assert!(Command::parse(vec![
            "merge".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "0".into(),
        ])
        .is_err());
    }

    #[test]
    fn rejects_zero_merge_loop_max_concurrent() {
        assert!(Command::parse(vec![
            "merge".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "1".into(),
            "--max-concurrent".into(),
            "0".into(),
        ])
        .is_err());
    }

    #[test]
    fn review_worker_selection_respects_concurrency_limit() {
        let selected = select_review_worker_issues(
            &[
                review_issue_with_ref("#67", "First review"),
                review_issue_with_ref("#68", "Second review"),
                review_issue_with_ref("#69", "Third review"),
            ],
            "Agent Review",
            "fake-reviewer",
            2,
        );

        assert_eq!(
            selected
                .iter()
                .map(|issue| issue.identifier.as_str())
                .collect::<Vec<_>>(),
            vec!["#67", "#68"]
        );
    }

    #[test]
    fn review_worker_selection_skips_existing_worker_marker() {
        let mut queued = review_issue_with_ref("#67", "Queued review");
        queued.project_fields.insert(
            "Review Worker".into(),
            serde_json::Value::String("queued review:#67:fake-reviewer".into()),
        );
        let ready = review_issue_with_ref("#68", "Ready review");

        let selected =
            select_review_worker_issues(&[queued, ready], "Agent Review", "fake-reviewer", 2);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].identifier, "#68");
    }

    #[test]
    fn review_worker_selection_skips_review_agent_field_claim() {
        let mut queued = review_issue_with_ref("#67", "Queued review");
        let claim = review_claim_for_issue(&queued, "review:#67:fake-reviewer");
        queued.project_fields.insert(
            "Review Agent".into(),
            serde_json::Value::String(claim.render()),
        );
        let ready = review_issue_with_ref("#68", "Ready review");

        let selected =
            select_review_worker_issues(&[queued, ready], "Agent Review", "fake-reviewer", 2);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].identifier, "#68");
    }

    #[test]
    fn review_claim_for_issue_replaces_terminal_review_claim() {
        let mut issue = review_issue_with_ref("#67", "Retry review");
        let terminal_claim = LaneClaim::active(
            "#67",
            LaneClaimLane::Review,
            LaneClaimActor::Gemini,
            LaneClaimSource::Loop,
            42,
        )
        .with_worker("review:#67:gemini-cli")
        .with_state(LaneClaimState::Failed);
        issue.project_fields.insert(
            "Review Agent".into(),
            serde_json::Value::String(format!("{} result=inconclusive", terminal_claim.render())),
        );

        let claim = review_claim_for_issue(&issue, "review:#67:gemini-cli");

        assert_eq!(claim.state, LaneClaimState::Active);
        assert_ne!(claim.run, terminal_claim.run);
    }

    #[test]
    fn review_loop_terminal_claim_records_pass_result() {
        let claim = LaneClaim::active(
            "#67",
            LaneClaimLane::Review,
            LaneClaimActor::Gemini,
            LaneClaimSource::Loop,
            42,
        )
        .with_worker("review:#67:gemini-cli");
        let decision = ReviewGateDecision {
            outcome: ReviewOutcome::PassedToHumanReview,
            target_state: Some("human_review"),
            message: "passed".into(),
        };
        let job = ReviewJob {
            id: "job".into(),
            issue_ref: "#67".into(),
            backend: "gemini-cli".into(),
            state: ReviewJobState::Completed,
            artifact_path: None,
            ledger_path: None,
            report: None,
            error: None,
        };

        let value = terminal_review_loop_claim_value(Some(&claim), &job, &decision).unwrap();

        assert!(value.contains("state=done"));
        assert!(value.contains("result=passed"));
        assert_eq!(
            LaneClaim::parse(&value).unwrap(),
            claim.with_state(LaneClaimState::Done)
        );
    }

    #[test]
    fn review_pass_checklist_update_checks_non_uat_sections_only() {
        let body = [
            "## Expected Outcome",
            "",
            "- [ ] Outcome done",
            "",
            "## Verification",
            "",
            "### Completion Criteria",
            "",
            "- [ ] Criteria done",
            "",
            "### Functional Verification",
            "",
            "- [ ] `cargo test`",
            "",
            "### UAT",
            "",
            "- [ ] Human checks this",
            "",
            "### Context Verification",
            "",
            "- [ ] Context done",
            "",
            "```md",
            "- [ ] do not touch fenced examples",
            "```",
        ]
        .join("\n");

        let updated = check_review_verified_issue_body_checkboxes(&body);

        assert!(updated.contains("- [x] Outcome done"));
        assert!(updated.contains("- [x] Criteria done"));
        assert!(updated.contains("- [x] `cargo test`"));
        assert!(updated.contains("- [ ] Human checks this"));
        assert!(updated.contains("- [x] Context done"));
        assert!(updated.contains("- [ ] do not touch fenced examples"));
    }

    #[test]
    fn review_pass_checklist_update_removes_appended_workpad_before_editing_body() {
        let description =
            "## Expected Outcome\n\n- [ ] Done\n\n<!-- jade-symphony-workpad -->\n## Agent Review";

        let body = canonical_issue_body_without_workpad(description);
        let updated = check_review_verified_issue_body_checkboxes(&body);

        assert_eq!(updated, "## Expected Outcome\n\n- [x] Done");
        assert!(!updated.contains("jade-symphony-workpad"));
    }

    #[test]
    fn review_pass_updates_issue_body_checkboxes_before_human_review_transition() {
        let config = test_config();
        let adapter = RecordingAdapter::default();
        let mut issue = review_issue_with_ref("#67", "Checklist review");
        issue.description = Some(
            [
                "## Expected Outcome",
                "",
                "- [ ] Outcome done",
                "",
                "## Verification",
                "",
                "### Completion Criteria",
                "",
                "- [ ] Criteria done",
                "",
                "### Functional Verification",
                "",
                "- [ ] `cargo test`",
                "",
                "### UAT",
                "",
                "- [ ] Human checks this",
                "",
                "### Context Verification",
                "",
                "- [ ] Context done",
            ]
            .join("\n"),
        );
        adapter
            .issues
            .borrow_mut()
            .insert(issue.identifier.clone(), issue.clone());
        let job = ReviewJob {
            id: "job-67".into(),
            issue_ref: "#67".into(),
            backend: "gemini-cli".into(),
            state: ReviewJobState::Completed,
            artifact_path: None,
            ledger_path: None,
            report: Some(jade_symphony::review::AgentReviewReport {
                summary: Some("Review Result: PASS".into()),
                ..Default::default()
            }),
            error: None,
        };

        apply_review_result(&config, &adapter, "#67", &issue, &job, None, None).unwrap();

        let updated = adapter
            .issues
            .borrow()
            .get("#67")
            .and_then(|issue| issue.description.clone())
            .unwrap();
        assert!(updated.contains("- [x] Outcome done"));
        assert!(updated.contains("- [x] Criteria done"));
        assert!(updated.contains("- [x] `cargo test`"));
        assert!(updated.contains("- [ ] Human checks this"));
        assert!(updated.contains("- [x] Context done"));
        assert_eq!(
            adapter.operations(),
            vec![
                "update_issue_content:#67",
                "comment:#67",
                "set_state:#67:human_review"
            ]
        );
    }

    #[test]
    fn review_workspace_uses_issue_handoff_workspace() {
        let config = test_config();
        let issue = review_issue_with_ref("#67", "Add parallel review worker pool");

        let workspace = review_workspace_for_issue(&config, &issue);

        assert!(workspace.ends_with("issue-67-add-parallel-review-worker-pool"));
    }

    #[test]
    fn parses_run_loop_flags() {
        let command = Command::parse(vec![
            "main".into(),
            "loop".into(),
            "examples/dry-run-workflow.md".into(),
            "--max-iterations".into(),
            "3".into(),
            "--max-concurrent".into(),
            "4".into(),
            "--display".into(),
            "tui".into(),
            "--dry-run".into(),
        ])
        .unwrap();

        let Command::RunLoop { options } = command else {
            panic!("expected main loop command");
        };

        assert_eq!(
            options.workflow_path,
            PathBuf::from("examples/dry-run-workflow.md")
        );
        assert_eq!(options.max_iterations, Some(3));
        assert_eq!(options.max_concurrent, Some(4));
        assert_eq!(options.worker_limit(&test_config()), 4);
        assert_eq!(options.display, DisplayMode::Tui);
        assert!(!options.once);
        assert!(!options.write);
        assert!(!options.recover);
    }

    #[test]
    fn run_loop_write_defaults_to_recover() {
        let command = Command::parse(vec![
            "main".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--write".into(),
        ])
        .unwrap();

        let Command::RunLoop { options } = command else {
            panic!("expected main loop command");
        };

        assert!(options.write);
        assert!(options.recover);
    }

    #[test]
    fn run_loop_no_recover_disables_write_default() {
        let command = Command::parse(vec![
            "main".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--write".into(),
            "--no-recover".into(),
        ])
        .unwrap();

        let Command::RunLoop { options } = command else {
            panic!("expected main loop command");
        };

        assert!(options.write);
        assert!(!options.recover);
    }

    #[test]
    fn run_loop_once_overrides_max_iterations() {
        let command = Command::parse(vec![
            "main".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "9".into(),
            "--once".into(),
            "--write".into(),
        ])
        .unwrap();

        let Command::RunLoop { options } = command else {
            panic!("expected main loop command");
        };

        assert_eq!(options.iteration_limit(), Some(1));
        assert!(options.write);
        assert!(options.recover);
    }

    #[test]
    fn parses_merge_once_command() {
        let command = Command::parse(vec![
            "merge".into(),
            "once".into(),
            "examples/github-project-workflow.md".into(),
            "--dry-run".into(),
        ])
        .unwrap();

        assert_eq!(
            command,
            Command::MergeOnce {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                write: false
            }
        );

        assert!(Command::parse(vec![
            "land".into(),
            "examples/github-project-workflow.md".into(),
            "--write".into()
        ])
        .is_err());
    }

    #[test]
    fn rejects_zero_run_loop_iterations() {
        let error = Command::parse(vec![
            "main".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "0".into(),
        ])
        .unwrap_err();

        assert!(error.contains("Usage:"));
    }

    #[test]
    fn rejects_zero_run_loop_max_concurrent() {
        let error = Command::parse(vec![
            "main".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "1".into(),
            "--max-concurrent".into(),
            "0".into(),
        ])
        .unwrap_err();

        assert!(error.contains("Usage:"));
    }

    #[test]
    fn pool_worker_selection_respects_lane_priority_and_claim_owner() {
        let config = test_config();
        let worker = "Jade Symphony Main";
        let mut first = tracker_issue_with_ref("#1", "First", "Todo");
        first.priority = Some(20);
        let mut second = tracker_issue_with_ref("#2", "Second", "Rework");
        second.priority = Some(10);
        let mut owned_by_other = tracker_issue_with_ref("#3", "Other owned", "Todo");
        owned_by_other.project_fields.insert(
            "Main Agent".into(),
            serde_json::Value::String("Another Main".into()),
        );
        let mut owned_by_self = tracker_issue_with_ref("#4", "Self owned", "In Progress");
        owned_by_self.priority = Some(5);
        owned_by_self.project_fields.insert(
            "Main Agent".into(),
            serde_json::Value::String(worker.into()),
        );
        let merging = tracker_issue_with_ref("#5", "Merging", "Merging");

        let selected = select_pool_worker_issues(
            &[first, second, owned_by_other, owned_by_self, merging],
            WorkerLane::Main,
            worker,
            2,
            &config,
        );

        assert_eq!(
            selected
                .iter()
                .map(|issue| issue.identifier.as_str())
                .collect::<Vec<_>>(),
            vec!["#4", "#2"]
        );
    }

    #[test]
    fn pool_worker_selection_returns_empty_when_no_slots_remain() {
        let config = test_config();
        let issue = tracker_issue_with_ref("#1", "Ready", "Todo");

        let selected = select_pool_worker_issues(&[issue], WorkerLane::Main, "worker", 0, &config);

        assert!(selected.is_empty());
    }

    #[test]
    fn main_run_loop_selection_prioritizes_recovery_and_fills_remaining_slots() {
        let config = test_config();
        let worker = "Jade Symphony Main";
        let recovery_issue = tracker_issue_with_ref("#362", "Recover me", "In Progress");
        let mut next_todo = tracker_issue_with_ref("#363", "Start next", "Todo");
        next_todo.priority = Some(1);
        let recovery = RuntimeRecoveryCandidate {
            state: active_runtime_state("#362"),
            issue: recovery_issue,
            reason: "retry_due attempt=2 error=HTTP 429".into(),
        };

        let selected = select_main_run_loop_issues(&[recovery], &[next_todo], 2, worker, &config);

        assert_eq!(
            selected
                .iter()
                .map(|issue| issue.identifier.as_str())
                .collect::<Vec<_>>(),
            vec!["#362", "#363"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn main_run_loop_write_dispatch_starts_selected_candidates_concurrently() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let codex = bin_dir.join("codex");
        let start_log = temp.path().join("codex-starts.log");
        std::fs::write(
            &codex,
            format!(
                r#"#!/bin/sh
set -eu
start_log={}
count=0
while IFS= read -r line; do
  count=$((count + 1))
  case "$count" in
    1)
      printf '%s\n' '{{"id":1,"result":{{}}}}'
      ;;
    2)
      ;;
    3)
      printf '{{"id":2,"result":{{"thread":{{"id":"thread-%s"}}}}}}\n' "$$"
      ;;
    4)
      printf '{{"id":3,"result":{{"turn":{{"id":"turn-%s"}}}}}}\n' "$$"
      printf '%s\n' "$$" >> "$start_log"
      remaining=40
      while [ "$remaining" -gt 0 ]; do
        starts="$(wc -l < "$start_log" 2>/dev/null || echo 0)"
        [ "$starts" -ge 2 ] && break
        remaining=$((remaining - 1))
        sleep 0.05
      done
      starts="$(wc -l < "$start_log" 2>/dev/null || echo 0)"
      if [ "$starts" -lt 2 ]; then
        printf '%s\n' '{{"method":"turn/failed","params":{{"error":{{"message":"second worker did not start before timeout"}}}}}}'
        exit 0
      fi
      printf '%s\n' '{{"method":"thread/tokenUsage/updated","params":{{"inputTokens":1,"outputTokens":1,"totalTokens":2}}}}'
      printf '%s\n' '{{"method":"turn/completed","params":{{"turn":{{"status":"completed"}}}}}}'
      exit 0
      ;;
  esac
done
"#,
                shell_quote_display(&start_log.display().to_string())
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&codex).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&codex, permissions).unwrap();

        let workflow_path = temp.path().join("WORKFLOW.md");
        let workflow = WorkflowDefinition::parse(
            &workflow_path,
            &format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {}\nartifacts:\n  root: {}\nobservability:\n  logs_root: {}\nmain_lane:\n  backend: codex\ncodex:\n  command: {} app-server\n  read_timeout_ms: 1000\n  turn_timeout_ms: 5000\n---\nPrompt for {{{{ issue.identifier }}}}",
                temp.path().join("worktrees").display(),
                temp.path().join("artifacts").display(),
                temp.path().join("logs").display(),
                shell_quote_display(&codex.display().to_string())
            ),
        )
        .unwrap();
        let config = RuntimeConfig::from_workflow(&workflow, &workflow_path).unwrap();
        let mut first = tracker_issue_with_ref("#362", "Recover first", "In Progress");
        first.id = "ISSUE_362".into();
        first.description = Some(forge_contract());
        let mut second = tracker_issue_with_ref("#363", "Start second", "In Progress");
        second.id = "ISSUE_363".into();
        second.description = Some(forge_contract());
        let options = RunLoopOptions {
            workflow_path,
            max_iterations: Some(1),
            once: false,
            write: true,
            recover: true,
            max_concurrent: Some(2),
            display: DisplayMode::Plain,
        };

        run_loop_dispatch_write_candidates(
            &workflow,
            &config,
            vec![first, second],
            &options,
            true,
            1,
            2,
        )
        .unwrap();

        let starts = std::fs::read_to_string(start_log).unwrap();
        assert_eq!(starts.lines().count(), 2);
    }

    #[test]
    fn merge_pool_selection_only_accepts_merging_lane() {
        let config = test_config();
        let mut claimed = tracker_issue_with_ref("#6", "Claimed merge", "Merging");
        claimed.project_fields.insert(
            "Merging Agent".into(),
            serde_json::Value::String("other merger".into()),
        );
        let mut unclaimed = tracker_issue_with_ref("#7", "Ready merge", "Merging");
        unclaimed.priority = Some(1);
        let todo = tracker_issue_with_ref("#8", "Main work", "Todo");

        let selected = select_pool_worker_issues(
            &[claimed, unclaimed, todo],
            WorkerLane::Merging,
            "this merger",
            4,
            &config,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].identifier, "#7");
    }

    #[test]
    fn merge_pool_selection_reuses_structured_active_claim_for_same_worker() {
        let config = test_config();
        let worker = "Jade Symphony Agent";
        let claim = LaneClaim::active(
            "#6",
            LaneClaimLane::Merge,
            LaneClaimActor::Codex,
            LaneClaimSource::Loop,
            1_779_000_000_000,
        )
        .with_worker(worker);
        let mut claimed_by_self = tracker_issue_with_ref("#6", "Claimed merge", "Merging");
        claimed_by_self.project_fields.insert(
            "Merging Agent".into(),
            serde_json::Value::String(claim.render()),
        );

        let selected =
            select_pool_worker_issues(&[claimed_by_self], WorkerLane::Merging, worker, 1, &config);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].identifier, "#6");
    }

    #[test]
    fn merge_recover_selection_prioritizes_interrupted_loop_claims() {
        let config = test_config();
        let worker = "Jade Symphony Agent";
        let interrupted_claim = LaneClaim::active(
            "#6",
            LaneClaimLane::Merge,
            LaneClaimActor::Codex,
            LaneClaimSource::Loop,
            1_779_000_000_000,
        )
        .with_worker("previous merger");
        let mut interrupted = tracker_issue_with_ref("#6", "Interrupted merge", "Merging");
        interrupted.priority = Some(20);
        interrupted.project_fields.insert(
            "Merging Agent".into(),
            serde_json::Value::String(interrupted_claim.render()),
        );
        let mut unclaimed = tracker_issue_with_ref("#7", "Ready merge", "Merging");
        unclaimed.priority = Some(1);

        let selected =
            select_merge_worker_issues(&[unclaimed, interrupted], worker, 1, &config, true);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].issue.identifier, "#6");
        assert!(selected[0]
            .recovery_reason
            .as_deref()
            .unwrap()
            .contains("previous_worker=previous merger"));
    }

    #[test]
    fn merge_recover_selection_does_not_adopt_manual_claims() {
        let config = test_config();
        let worker = "Jade Symphony Agent";
        let manual_claim = LaneClaim::active(
            "#6",
            LaneClaimLane::Merge,
            LaneClaimActor::Codex,
            LaneClaimSource::Manual,
            1_779_000_000_000,
        )
        .with_worker("manual merger");
        let mut manual = tracker_issue_with_ref("#6", "Manual merge", "Merging");
        manual.project_fields.insert(
            "Merging Agent".into(),
            serde_json::Value::String(manual_claim.render()),
        );
        let unclaimed = tracker_issue_with_ref("#7", "Ready merge", "Merging");

        let selected = select_merge_worker_issues(&[manual, unclaimed], worker, 2, &config, true);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].issue.identifier, "#7");
        assert!(selected[0].recovery_reason.is_none());
    }

    #[test]
    fn pool_claim_eligibility_reports_existing_owner() {
        let config = test_config();
        let mut issue = tracker_issue("Todo");
        issue.project_fields.insert(
            "Main Agent".into(),
            serde_json::Value::String("someone else".into()),
        );

        assert_eq!(
            pool_claim_eligibility(&issue, WorkerLane::Main, "this worker", &config),
            PoolClaimEligibility::ClaimedByOther {
                owner: "someone else".into()
            }
        );
    }

    #[test]
    fn rejects_zero_review_loop_iterations() {
        let error = Command::parse(vec![
            "review".into(),
            "loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "0".into(),
        ])
        .unwrap_err();

        assert!(error.contains("Usage:"));
    }

    #[test]
    fn run_loop_claim_action_uses_tracker_claim_decision() {
        let config = test_config();

        assert_eq!(
            run_loop_claim_action(&tracker_issue("Todo"), &config),
            RunLoopClaimAction::Claim
        );
        assert_eq!(
            run_loop_claim_action(&tracker_issue("Rework"), &config),
            RunLoopClaimAction::Claim
        );
        assert_eq!(
            run_loop_claim_action(&tracker_issue("In Progress"), &config),
            RunLoopClaimAction::Resume
        );
        assert_eq!(
            run_loop_claim_action(&tracker_issue("Agent Review"), &config),
            RunLoopClaimAction::StopAndReplan {
                current_state: "Agent Review".into()
            }
        );
    }

    #[test]
    fn live_gate_blocks_missing_assignee_without_override() {
        let config = live_github_config(false);
        let issue = tracker_issue("Todo");

        assert_eq!(
            live_missing_assignee_gate_blocker(&config, &issue).as_deref(),
            Some("live GitHub issue assignee")
        );
    }

    #[test]
    fn issue_contract_assignees_parse_setup_field() {
        assert_eq!(
            issue_contract_assignees("- Assignee: @Alive24\n- UAT Required: Yes"),
            vec!["Alive24".to_string()]
        );
        assert_eq!(
            issue_contract_assignees("- Assignees: Alive24, codex\n"),
            vec!["Alive24".to_string(), "codex".to_string()]
        );
    }

    #[test]
    fn fixture_mode_does_not_require_live_assignee() {
        let config = fixture_github_config();
        let issue = tracker_issue("Todo");

        assert_eq!(live_missing_assignee_gate_blocker(&config, &issue), None);
        assert_eq!(
            run_loop_assignee_ownership_decision(&issue, &config, None, None),
            AssigneeOwnershipDecision::Allowed
        );
    }

    #[test]
    fn assignee_ownership_allows_matching_active_login() {
        let config = live_github_config(false);
        let mut issue = tracker_issue("Todo");
        issue.assignees = vec!["CodexUser".into()];

        assert_eq!(
            run_loop_assignee_ownership_decision(&issue, &config, Some("codexuser"), None),
            AssigneeOwnershipDecision::Allowed
        );
    }

    #[test]
    fn assignee_ownership_blocks_mismatched_active_login() {
        let config = live_github_config(false);
        let mut issue = tracker_issue("Todo");
        issue.assignees = vec!["owner-a".into()];

        let decision = run_loop_assignee_ownership_decision(&issue, &config, Some("owner-b"), None);

        assert!(matches!(decision, AssigneeOwnershipDecision::Block { .. }));
    }

    #[test]
    fn assignee_ownership_allows_matching_profile_login() {
        let config = live_github_config(false);
        let mut issue = tracker_issue("Todo");
        issue.assignees = vec!["profile-owner".into()];

        assert_eq!(
            run_loop_assignee_ownership_decision(
                &issue,
                &config,
                Some("different-gh-user"),
                Some("profile-owner"),
            ),
            AssigneeOwnershipDecision::Allowed
        );
    }

    #[test]
    fn assignee_ownership_blocks_missing_active_identity() {
        let config = live_github_config(false);
        let mut issue = tracker_issue("Todo");
        issue.assignees = vec!["owner-a".into()];

        let decision = run_loop_assignee_ownership_decision(&issue, &config, None, None);

        assert_eq!(
            decision,
            AssigneeOwnershipDecision::Block {
                reason: "active GitHub identity unavailable for assignee ownership check".into(),
            }
        );
    }

    #[test]
    fn run_loop_runtime_ownership_workpad_records_matching_marker() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let ownership = run_loop_runtime_ownership(&issue, &config, &handoff).unwrap();
        let claim = test_claim(&issue);

        let workpad = run_loop_ownership_workpad(&issue, &ownership, "Resumed", &claim);

        assert!(workpad.contains("jade-symphony-runtime-ownership"));
        assert_eq!(
            runtime_ownership_decision(Some(&workpad), &ownership),
            RuntimeOwnershipDecision::Matches
        );
    }

    #[test]
    fn run_loop_runtime_ownership_detects_different_active_branch() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let expected = run_loop_runtime_ownership(&issue, &config, &handoff).unwrap();
        let mut existing = expected.clone();
        existing.branch_name = "feature/issue-100-other-work".into();
        let workpad = render_runtime_ownership_marker(&existing);

        assert!(matches!(
            runtime_ownership_decision(Some(&workpad), &expected),
            RuntimeOwnershipDecision::Mismatched { .. }
        ));
    }

    #[test]
    fn resume_preflight_continues_active_in_progress_state() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let state = active_runtime_state("#29");

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

        assert_eq!(action, ResumePreflightAction::Continue);
    }

    #[test]
    fn resume_preflight_archives_non_active_state_with_absent_worktree() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("Need to Clarify")]);
        let state = active_runtime_state("#29");

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

        assert_eq!(
            action,
            ResumePreflightAction::ArchiveStale {
                issue_identifier: "#29".into(),
                tracker_state: "Need to Clarify".into(),
                archive_reason: "tracker_state_non_active".into(),
            }
        );
    }

    #[test]
    fn resume_preflight_archives_terminal_state_with_clean_worktree() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("Done")]);
        let temp = tempfile::tempdir().unwrap();
        init_clean_git_workspace(temp.path());
        let mut state = active_runtime_state("#29");
        state.workspace_path = Some(temp.path().to_path_buf());

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

        assert_eq!(
            action,
            ResumePreflightAction::ArchiveStale {
                issue_identifier: "#29".into(),
                tracker_state: "Done".into(),
                archive_reason: "tracker_state_terminal".into(),
            }
        );
    }

    #[test]
    fn resume_preflight_blocks_non_active_state_with_dirty_worktree() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("Need to Clarify")]);
        let temp = tempfile::tempdir().unwrap();
        init_clean_git_workspace(temp.path());
        std::fs::write(temp.path().join("scratch.txt"), "dirty work").unwrap();
        let mut state = active_runtime_state("#29");
        state.workspace_path = Some(temp.path().to_path_buf());

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

        assert!(
            matches!(action, ResumePreflightAction::Block { reason } if reason.contains("workspace is dirty"))
        );
    }

    #[test]
    fn resume_preflight_archive_allows_unrelated_todo_selection() {
        let config = main_loop_test_config();
        let stale = tracker_issue_with_ref("#29", "Needs clarification", "Need to Clarify");
        let mut todo = tracker_issue_with_ref("#30", "Ready next work", "Todo");
        todo.description = Some(forge_contract());
        let tracker = MemoryTracker::new(vec![stale, todo.clone()]);
        let state = active_runtime_state("#29");

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();
        let plan =
            Orchestrator::new(config).plan_dispatch(tracker.list_dispatchable_issues().unwrap());

        assert!(matches!(action, ResumePreflightAction::ArchiveStale { .. }));
        assert_eq!(
            plan.selected.first().map(|issue| issue.identifier.as_str()),
            Some("#30")
        );
    }

    #[test]
    fn resume_preflight_many_counts_active_main_worker_slots() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        let tracker = MemoryTracker::new(vec![
            tracker_issue_with_ref("#29", "Runtime one", "In Progress"),
            tracker_issue_with_ref("#30", "Runtime two", "In Progress"),
        ]);
        let states = vec![active_runtime_state("#29"), active_runtime_state("#30")];

        let summary =
            run_loop_resume_preflight_many(&tracker, &config, &states, 2_000, false).unwrap();

        assert_eq!(summary.active_main_workers, 2);
        assert_eq!(summary.retained_states.len(), 2);
        assert_eq!(summary.blocked, None);
    }

    #[test]
    fn resume_preflight_many_archives_only_stale_slot() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        let tracker = MemoryTracker::new(vec![
            tracker_issue_with_ref("#29", "Handed off", "Agent Review"),
            tracker_issue_with_ref("#30", "Still active", "In Progress"),
        ]);
        let states = vec![active_runtime_state("#29"), active_runtime_state("#30")];

        let summary =
            run_loop_resume_preflight_many(&tracker, &config, &states, 2_000, false).unwrap();

        assert_eq!(summary.active_main_workers, 1);
        assert_eq!(summary.retained_states.len(), 1);
        assert_eq!(
            summary.retained_states[0]
                .active_issue
                .as_ref()
                .map(|issue| issue.identifier.as_str()),
            Some("#30")
        );
    }

    #[test]
    fn resume_preflight_many_marks_stalled_state_recoverable_when_requested() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut state = active_runtime_state("#29");
        state.updated_at_ms = Some(1_000);

        let summary = run_loop_resume_preflight_many(
            &tracker,
            &config,
            &[state],
            config.codex.stall_timeout_ms + 2_000,
            true,
        )
        .unwrap();

        assert_eq!(summary.active_main_workers, 0);
        assert_eq!(summary.blocked, None);
        assert_eq!(summary.retained_states.len(), 1);
        assert_eq!(summary.recoverable_states.len(), 1);
    }

    #[test]
    fn resume_preflight_many_prefers_completed_app_server_session_over_stale_runtime_clock() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut state = active_runtime_state("#29");
        state.backend = "codex".into();
        state.last_event = Some("Resumed".into());
        state.backend_session_id = None;
        state.updated_at_ms = Some(1_000);

        let mut record = main_tmux_session_record("#29", SessionStatus::Completed);
        record.backend = "codex".into();
        record.session_source = Some("codex-app-server".into());
        record.session_name = "thread-29-turn-1".into();
        record.pane_target = String::new();
        record.log_path = temp.path().join("logs/app-server/29.events.json");
        record.updated_at_ms = 2_000;
        save_session_registry(
            &session_registry_path(&config),
            &jade_symphony::session_registry::SessionRegistry {
                sessions: vec![record],
            },
        )
        .unwrap();

        let summary = run_loop_resume_preflight_many(
            &tracker,
            &config,
            &[state],
            config.codex.stall_timeout_ms + 2_000,
            true,
        )
        .unwrap();

        assert_eq!(summary.active_main_workers, 0);
        assert_eq!(summary.blocked, None);
        assert_eq!(summary.recoverable_states.len(), 1);
        assert!(summary.recoverable_states[0]
            .reason
            .contains("status=completed"));
        assert_eq!(
            summary.recoverable_states[0].state.last_event.as_deref(),
            Some("SessionTerminal")
        );
        assert_eq!(
            summary.recoverable_states[0]
                .state
                .backend_session_id
                .as_deref(),
            Some("thread-29-turn-1")
        );
    }

    #[test]
    fn resumed_pending_session_state_preserves_backend_session_for_reconciliation() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let claim = test_claim(&issue);
        let mut existing = active_runtime_state(&issue.identifier);
        existing.backend = "codex".into();
        existing.last_event = Some("SessionTerminal".into());
        existing.backend_session_id = Some("thread-29-turn-1".into());
        existing.backend_log_path = Some(PathBuf::from("/tmp/29.events.json"));
        existing.workspace_path = Some(PathBuf::from("/tmp/issue-29"));

        let state =
            run_loop_runtime_state_for_issue(Some(&existing), &issue, &config, "Resumed", &claim);

        assert_eq!(state.last_event.as_deref(), Some("SessionTerminal"));
        assert_eq!(
            state.backend_session_id.as_deref(),
            Some("thread-29-turn-1")
        );
        assert_eq!(state.attempt_count, existing.attempt_count);
    }

    #[test]
    fn resume_preflight_many_marks_missing_session_registry_recoverable() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut state = active_runtime_state("#29");
        state.backend = "tmux".into();
        state.last_event = Some("SessionRunning".into());
        state.backend_session_id = Some("jade-main-missing".into());

        let summary =
            run_loop_resume_preflight_many(&tracker, &config, &[state], 2_000, true).unwrap();

        assert_eq!(summary.active_main_workers, 0);
        assert_eq!(summary.blocked, None);
        assert_eq!(summary.recoverable_states.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn resume_preflight_many_counts_running_tmux_session_in_recover_mode() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        config.tmux.command = fake_tmux_capture_script(
            temp.path(),
            "Codex\n◦ Running cargo run -- autopilot plan\n› Improve documentation in @filename",
        );
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut state = active_runtime_state("#29");
        state.backend = "tmux".into();
        state.last_event = Some("SessionRunning".into());
        state.backend_session_id = Some("jade-main-29-attempt-1".into());
        state.updated_at_ms = Some(1_000);

        let mut record = main_tmux_session_record("#29", SessionStatus::Running);
        record.session_name = "jade-main-29-attempt-1".into();
        record.pane_target = "jade-main-29-attempt-1".into();
        record.log_path = temp.path().join("session.log");
        record.updated_at_ms = 1_000;
        save_session_registry(
            &session_registry_path(&config),
            &jade_symphony::session_registry::SessionRegistry {
                sessions: vec![record],
            },
        )
        .unwrap();

        let summary = run_loop_resume_preflight_many(
            &tracker,
            &config,
            &[state],
            config.codex.stall_timeout_ms + 2_000,
            true,
        )
        .unwrap();

        assert_eq!(summary.active_main_workers, 1);
        assert_eq!(summary.blocked, None);
        assert_eq!(summary.retained_states.len(), 1);
        assert_eq!(summary.recoverable_states.len(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn resume_preflight_many_counts_registry_only_running_tmux_session_in_recover_mode() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        config.tmux.command = fake_tmux_capture_script(
            temp.path(),
            "Codex\n◦ Running cargo test\n› Improve documentation in @filename",
        );
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut record = main_tmux_session_record("#29", SessionStatus::Running);
        record.session_name = "jade-main-29-attempt-1".into();
        record.pane_target = "jade-main-29-attempt-1".into();
        record.log_path = temp.path().join("session.log");
        record.updated_at_ms = 1_000;
        save_session_registry(
            &session_registry_path(&config),
            &jade_symphony::session_registry::SessionRegistry {
                sessions: vec![record],
            },
        )
        .unwrap();

        let summary = run_loop_resume_preflight_many(
            &tracker,
            &config,
            &[],
            config.codex.stall_timeout_ms + 2_000,
            true,
        )
        .unwrap();

        assert_eq!(summary.active_main_workers, 1);
        assert_eq!(summary.blocked, None);
        assert_eq!(summary.retained_states.len(), 1);
        assert_eq!(summary.recoverable_states.len(), 0);
        assert_eq!(
            runtime_state_issue_identifier(&summary.retained_states[0]),
            Some("#29")
        );
    }

    #[test]
    fn resume_preflight_many_recovers_registry_only_failed_app_server_session() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut record = main_tmux_session_record("#29", SessionStatus::Failed);
        record.backend = "codex".into();
        record.session_source = Some("codex-app-server".into());
        record.session_name = "thread-29-turn-1".into();
        record.pane_target = String::new();
        record.attach_command =
            "not a tmux session; inspect app-server artifacts for recovery evidence".into();
        record.log_path = temp.path().join("logs/app-server/29.events.json");
        save_session_registry(
            &session_registry_path(&config),
            &jade_symphony::session_registry::SessionRegistry {
                sessions: vec![record],
            },
        )
        .unwrap();

        let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

        assert_eq!(summary.active_main_workers, 0);
        assert_eq!(summary.blocked, None);
        assert_eq!(summary.retained_states.len(), 1);
        assert_eq!(summary.recoverable_states.len(), 1);
        assert!(summary.recoverable_states[0]
            .reason
            .contains("status=failed"));
        assert_eq!(
            runtime_state_issue_identifier(&summary.retained_states[0]),
            Some("#29")
        );
    }

    #[cfg(unix)]
    #[test]
    fn resume_preflight_registry_active_session_does_not_require_live_issue_read() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        config.tmux.command = fake_tmux_capture_script(
            temp.path(),
            "Codex\n◦ Running cargo test\n› Improve documentation in @filename",
        );
        let tracker = RecordingAdapter {
            fail_get_issue: true,
            ..Default::default()
        };
        let mut record = main_tmux_session_record("#29", SessionStatus::Running);
        record.session_name = "jade-main-29-attempt-1".into();
        record.pane_target = "jade-main-29-attempt-1".into();
        record.log_path = temp.path().join("session.log");
        save_session_registry(
            &session_registry_path(&config),
            &jade_symphony::session_registry::SessionRegistry {
                sessions: vec![record],
            },
        )
        .unwrap();

        let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

        assert_eq!(summary.active_main_workers, 1);
        assert_eq!(summary.recoverable_states.len(), 0);
        assert_eq!(summary.retained_states.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn resume_preflight_registry_active_session_skips_non_in_progress_tracker_state() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        config.tmux.command = fake_tmux_capture_script(
            temp.path(),
            "Codex\n◦ Running cargo test\n› Improve documentation in @filename",
        );
        let tracker = MemoryTracker::new(vec![tracker_issue("Agent Review")]);
        let mut record = main_tmux_session_record("#29", SessionStatus::Running);
        record.session_name = "jade-main-29-attempt-1".into();
        record.pane_target = "jade-main-29-attempt-1".into();
        record.log_path = temp.path().join("session.log");
        save_session_registry(
            &session_registry_path(&config),
            &jade_symphony::session_registry::SessionRegistry {
                sessions: vec![record],
            },
        )
        .unwrap();

        let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

        assert_eq!(summary.active_main_workers, 0);
        assert_eq!(summary.recoverable_states.len(), 0);
        assert_eq!(summary.retained_states.len(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn resume_preflight_prefers_running_sibling_session_over_interrupted_runtime_session() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        config.tmux.command = fake_tmux_split_session_script(temp.path());
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut attempt_two = main_tmux_session_record("#29", SessionStatus::Running);
        attempt_two.session_name = "jade-main-29-attempt-2".into();
        attempt_two.pane_target = "jade-main-29-attempt-2".into();
        attempt_two.log_path = temp.path().join("attempt-2.log");
        attempt_two.attempt = 2;
        let mut attempt_three = main_tmux_session_record("#29", SessionStatus::Running);
        attempt_three.session_name = "jade-main-29-attempt-3".into();
        attempt_three.pane_target = "jade-main-29-attempt-3".into();
        attempt_three.log_path = temp.path().join("attempt-3.log");
        attempt_three.attempt = 3;
        save_session_registry(
            &session_registry_path(&config),
            &jade_symphony::session_registry::SessionRegistry {
                sessions: vec![attempt_two, attempt_three],
            },
        )
        .unwrap();
        let mut state = active_runtime_state("#29");
        state.backend = "tmux".into();
        state.last_event = Some("SessionRunning".into());
        state.backend_session_id = Some("jade-main-29-attempt-3".into());
        state.updated_at_ms = Some(1_000);

        let summary = run_loop_resume_preflight_many(
            &tracker,
            &config,
            &[state],
            config.codex.stall_timeout_ms + 2_000,
            true,
        )
        .unwrap();

        assert_eq!(summary.active_main_workers, 1);
        assert_eq!(summary.recoverable_states.len(), 0);
        assert_eq!(
            summary.retained_states[0].backend_session_id.as_deref(),
            Some("jade-main-29-attempt-2")
        );
    }

    #[cfg(unix)]
    #[test]
    fn resume_preflight_many_recovers_registry_only_unavailable_tmux_session() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        config.tmux.command = fake_tmux_unavailable_script(temp.path());
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut record = main_tmux_session_record("#29", SessionStatus::Running);
        record.session_name = "jade-main-29-attempt-1".into();
        record.pane_target = "jade-main-29-attempt-1".into();
        record.log_path = temp.path().join("session.log");
        save_session_registry(
            &session_registry_path(&config),
            &jade_symphony::session_registry::SessionRegistry {
                sessions: vec![record],
            },
        )
        .unwrap();

        let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

        assert_eq!(summary.active_main_workers, 0);
        assert_eq!(summary.blocked, None);
        assert_eq!(summary.retained_states.len(), 1);
        assert_eq!(summary.recoverable_states.len(), 1);
        assert!(summary.recoverable_states[0]
            .reason
            .contains("registry_session_unavailable"));
    }

    #[cfg(unix)]
    #[test]
    fn resume_preflight_many_recovers_runtime_state_unavailable_tmux_session() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        config.tmux.command = fake_tmux_unavailable_script(temp.path());
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut record = main_tmux_session_record("#29", SessionStatus::Running);
        record.session_name = "jade-main-29-attempt-1".into();
        record.pane_target = "jade-main-29-attempt-1".into();
        record.log_path = temp.path().join("session.log");
        save_session_registry(
            &session_registry_path(&config),
            &jade_symphony::session_registry::SessionRegistry {
                sessions: vec![record],
            },
        )
        .unwrap();
        let mut state = active_runtime_state("#29");
        state.backend = "tmux".into();
        state.last_event = Some("SessionRunning".into());
        state.backend_session_id = Some("jade-main-29-attempt-1".into());

        let summary =
            run_loop_resume_preflight_many(&tracker, &config, &[state], 2_000, true).unwrap();

        assert_eq!(summary.active_main_workers, 0);
        assert_eq!(summary.blocked, None);
        assert_eq!(summary.retained_states.len(), 1);
        assert_eq!(summary.recoverable_states.len(), 1);
        assert!(summary.recoverable_states[0]
            .reason
            .contains("tmux_pane_unavailable"));
    }

    #[cfg(unix)]
    fn fake_tmux_capture_script(root: &Path, output: &str) -> String {
        use std::os::unix::fs::PermissionsExt;

        let path = root.join("fake-tmux");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"${{1:-}}\" = \"capture-pane\" ]; then\ncat <<'EOF'\n{output}\nEOF\nexit 0\nfi\nexit 0\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    #[cfg(unix)]
    fn fake_tmux_split_session_script(root: &Path) -> String {
        use std::os::unix::fs::PermissionsExt;

        let path = root.join("fake-tmux-split-session");
        std::fs::write(
            &path,
            r#"#!/bin/sh
case "$*" in
  *attempt-2*) printf '%s\n' 'Codex' '◦ Running cargo test' '› Improve documentation in @filename'; exit 0 ;;
  *attempt-3*) printf '%s\n' 'Conversation interrupted - tell the model what to do differently.' '› Write tests for @filename'; exit 0 ;;
esac
exit 1
"#,
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    #[cfg(unix)]
    fn fake_tmux_unavailable_script(root: &Path) -> String {
        use std::os::unix::fs::PermissionsExt;

        let path = root.join("fake-tmux-unavailable");
        std::fs::write(
            &path,
            "#!/bin/sh\nif [ \"${1:-}\" = \"capture-pane\" ]; then\nexit 1\nfi\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    #[test]
    fn recovery_handoff_reuses_dirty_existing_issue_worktree() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.workspace.root = temp.path().join("worktrees");
        std::fs::create_dir_all(&config.workspace.root).unwrap();
        let issue = tracker_issue("In Progress");
        let mut handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let worktree = config.workspace.root.join("_29-main-agent");
        init_clean_git_workspace(&worktree);
        git_ok(
            &worktree,
            &["checkout", "-b", "feature/issue-29-runtime-state-main-loop"],
        );
        std::fs::write(worktree.join("scratch.txt"), "dirty recovery work").unwrap();
        let mut state = active_runtime_state("#29");
        state.last_event = Some("SessionRunning".into());
        state.workspace_path = Some(worktree.clone());

        let evidence =
            run_loop_apply_recovery_handoff(&config, &issue, &mut handoff, &state).unwrap();

        assert!(evidence
            .as_deref()
            .unwrap()
            .contains("source=runtime_state"));
        assert_eq!(handoff.workspace_path, worktree);
        assert_eq!(handoff.workspace_key, "_29-main-agent");
        assert_eq!(
            handoff.branch_name,
            "feature/issue-29-runtime-state-main-loop"
        );
    }

    #[test]
    fn run_loop_runtime_state_uses_matching_slot_for_attempt_count() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let claim = test_claim(&issue);
        let unrelated = active_runtime_state("#28");
        let existing = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
        let states = vec![unrelated, existing];

        let state = run_loop_runtime_state_for_issue(
            runtime_state_for_issue(&states, &issue.identifier),
            &issue,
            &config,
            "Resumed",
            &claim,
        );

        assert_eq!(state.attempt_count, 2);
        assert_eq!(
            state
                .active_issue
                .as_ref()
                .map(|issue| issue.identifier.as_str()),
            Some("#29")
        );
    }

    #[test]
    fn resume_preflight_defers_until_retry_is_due() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut state = active_runtime_state("#29");
        record_runtime_retry(&mut state, 1_000, 5_000, "rate limited");

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

        assert!(matches!(
            action,
            ResumePreflightAction::RetryLater {
                due_in_ms: 4_000,
                ..
            }
        ));
    }

    #[test]
    fn resume_preflight_continues_after_retry_is_due_even_when_old() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut state = active_runtime_state("#29");
        record_runtime_retry(&mut state, 1_000, 5_000, "backend not ready");

        let action = run_loop_resume_preflight(
            &tracker,
            &config,
            Some(&state),
            config.codex.stall_timeout_ms + 10_000,
        )
        .unwrap();

        assert_eq!(action, ResumePreflightAction::Continue);
    }

    #[test]
    fn resume_preflight_many_marks_due_retry_recoverable_when_requested() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut state = active_runtime_state("#29");
        record_runtime_retry(&mut state, 1_000, 5_000, "HTTP 429 too many requests");

        let summary =
            run_loop_resume_preflight_many(&tracker, &config, &[state], 7_000, true).unwrap();

        assert_eq!(summary.active_main_workers, 0);
        assert_eq!(summary.recoverable_states.len(), 1);
        assert!(summary.recoverable_states[0]
            .reason
            .contains("retry_due attempt="));
    }

    #[test]
    fn resume_preflight_detects_stalled_active_state() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut state = active_runtime_state("#29");
        state.updated_at_ms = Some(1_000);

        let action = run_loop_resume_preflight(
            &tracker,
            &config,
            Some(&state),
            config.codex.stall_timeout_ms + 2_000,
        )
        .unwrap();

        assert!(matches!(action, ResumePreflightAction::Stalled { .. }));
    }

    #[test]
    fn resume_preflight_archives_completed_tracker_state() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("Agent Review")]);
        let state = active_runtime_state("#29");

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

        assert_eq!(
            action,
            ResumePreflightAction::ArchiveStale {
                issue_identifier: "#29".into(),
                tracker_state: "Agent Review".into(),
                archive_reason: "tracker_state_handoff".into(),
            }
        );
    }

    #[test]
    fn main_handoff_reconcile_completes_session_and_clears_matching_runtime_state() {
        let temp = tempfile::tempdir().unwrap();
        let config = runtime_reconcile_test_config(temp.path());
        let mut state = active_runtime_state("#338");
        state.backend = "tmux".into();
        state.backend_session_id = Some("jade-main-338-attempt-1-reconcile".into());
        state.lane = Some("main".into());
        upsert_runtime_state(&config, &state).unwrap();
        save_session_registry(
            &session_registry_path(&config),
            &jade_symphony::session_registry::SessionRegistry {
                sessions: vec![main_tmux_session_record("#338", SessionStatus::Running)],
            },
        )
        .unwrap();

        reconcile_main_handoff_runtime_state(&config, "#338", "agent_review").unwrap();

        let runtime_states = load_runtime_states(&config).unwrap();
        assert!(runtime_state_for_issue(&runtime_states, "#338").is_none());
        let registry = load_session_registry(&session_registry_path(&config)).unwrap();
        assert_eq!(registry.sessions[0].status, SessionStatus::Completed);
        assert!(registry.sessions[0].updated_at_ms > 1_000);
    }

    #[test]
    fn main_handoff_reconcile_does_not_clear_non_main_runtime_state() {
        let temp = tempfile::tempdir().unwrap();
        let config = runtime_reconcile_test_config(temp.path());
        let mut state = active_runtime_state("#338");
        state.backend = "tmux".into();
        state.backend_session_id = Some("jade-review-338-attempt-1-review".into());
        state.lane = Some("review".into());
        upsert_runtime_state(&config, &state).unwrap();

        reconcile_main_handoff_runtime_state(&config, "#338", "agent_review").unwrap();

        let runtime_states = load_runtime_states(&config).unwrap();
        assert_eq!(
            runtime_state_for_issue(&runtime_states, "#338"),
            Some(&state)
        );
    }

    #[test]
    fn run_loop_runtime_state_increments_same_issue_attempts() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let claim = test_claim(&issue);
        let existing = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);

        let state =
            run_loop_runtime_state_for_issue(Some(&existing), &issue, &config, "Resumed", &claim);

        assert_eq!(state.attempt_count, 2);
        assert_eq!(
            state
                .active_issue
                .as_ref()
                .map(|issue| issue.identifier.as_str()),
            Some("#29")
        );
        assert_eq!(state.branch_name, issue.branch_name);
        assert_eq!(state.actor_role.as_deref(), Some("implementation_agent"));
        assert_eq!(state.actor_label.as_deref(), Some("Jade Symphony Agent"));
        assert_eq!(state.last_event.as_deref(), Some("Resumed"));
    }

    #[test]
    fn run_loop_runtime_state_records_result_and_transition() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let claim = test_claim(&issue);
        let state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
        let result = IssueExecutionResult {
            workspace_path: PathBuf::from("/tmp/jade/issue-29"),
            backend: "dry-run".into(),
            profile_id: Some("codex-alpha".into()),
            instance_name: Some("Codex Alpha".into()),
            success: true,
            pending_session: false,
            session_id: Some("session-29".into()),
            run_id: Some(claim.run.clone()),
            backend_log_path: None,
            backend_attach_command: None,
            message: "ok".into(),
            usage_limit_pause: None,
            prompt_artifact_path: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
                author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
                applied_keys: vec!["user.name".into(), "user.email".into()],
            },
            live_handoff: None,
            handoff_verification: None,
        };

        let state = run_loop_runtime_state_with_result(state, &result);
        assert_eq!(state.workspace_path, Some(result.workspace_path));
        assert_eq!(state.backend_session_id.as_deref(), Some("session-29"));
        assert_eq!(state.profile_id.as_deref(), Some("codex-alpha"));
        assert_eq!(state.actor_role.as_deref(), Some("implementation_agent"));
        assert_eq!(
            state.git_author.as_deref(),
            Some("Jade Symphony Agent <jade@example.invalid>")
        );
        assert_eq!(state.last_event.as_deref(), Some("Completed"));

        let state = run_loop_runtime_state_with_transition(
            state,
            Some("In Progress".into()),
            "agent_review",
            "main agent completed",
        );
        assert_eq!(
            state.last_transition,
            Some(RuntimeTransition {
                from: Some("In Progress".into()),
                to: "agent_review".into(),
                reason: "main agent completed".into(),
            })
        );
    }

    #[test]
    fn run_loop_runtime_state_records_pending_tmux_session_metadata() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let claim = test_claim(&issue);
        let state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
        let result = IssueExecutionResult {
            workspace_path: PathBuf::from("/tmp/jade/issue-220"),
            backend: "tmux".into(),
            profile_id: None,
            instance_name: None,
            success: false,
            pending_session: true,
            session_id: Some("jade-main-220".into()),
            run_id: Some(claim.run.clone()),
            backend_log_path: Some(PathBuf::from("/tmp/jade/logs/tmux/jade-main-220.log")),
            backend_attach_command: Some("tmux attach-session -t jade-main-220".into()),
            message: "tmux session running".into(),
            usage_limit_pause: None,
            prompt_artifact_path: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: None,
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::NotGitRepository,
                author: None,
                applied_keys: Vec::new(),
            },
            live_handoff: None,
            handoff_verification: None,
        };

        let state = run_loop_runtime_state_with_result(state, &result);
        let workpad = run_loop_handoff_workpad(
            &issue,
            &result,
            &run_loop_handoff_plan(&config, &issue).unwrap(),
            None,
        );

        assert_eq!(state.last_event.as_deref(), Some("SessionRunning"));
        assert_eq!(state.backend_session_id.as_deref(), Some("jade-main-220"));
        assert_eq!(
            state.backend_attach_command.as_deref(),
            Some("tmux attach-session -t jade-main-220")
        );
        assert!(workpad.contains("Session status: `running`"));
        assert!(workpad.contains("Attach command: `tmux attach-session -t jade-main-220`"));
        assert!(workpad.contains("Session log: `/tmp/jade/logs/tmux/jade-main-220.log`"));
    }

    #[test]
    fn main_loop_reconciles_completed_pending_session_without_relaunching_backend() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = main_loop_test_config();
        config.artifacts.root = temp.path().join("artifacts");
        config.observability.logs_root = temp.path().join("logs");
        let issue = tracker_issue("In Progress");
        let claim = test_claim(&issue);
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let mut state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
        state.last_event = Some("SessionRunning".into());
        state.workspace_path = Some(handoff.workspace_path.clone());
        state.backend = "tmux".into();
        state.backend_session_id = Some("jade-main-29".into());
        state.backend_attach_command = Some("tmux attach-session -t jade-main-29".into());
        state.backend_log_path = Some(temp.path().join("jade-main-29.log"));

        save_session_record(
            &session_registry_path(&config),
            AgentSessionRecord {
                issue_id: Some(issue.id.clone()),
                issue_identifier: Some(issue.identifier.clone()),
                issue_title: Some(issue.title.clone()),
                lane: "main".into(),
                run_id: state.run_id.clone(),
                thread: None,
                session_source: None,
                claim_value: None,
                actor_role: state.actor_role.clone(),
                actor_label: state.actor_label.clone(),
                git_author: state.git_author.clone(),
                profile_id: state.profile_id.clone(),
                instance_name: state.instance_name.clone(),
                worktree: handoff.workspace_path.clone(),
                branch: Some(handoff.branch_name.clone()),
                backend: "codex".into(),
                session_name: "jade-main-29".into(),
                pane_target: String::new(),
                prompt_artifact_path: temp.path().join("prompt.md"),
                log_path: temp.path().join("jade-main-29.log"),
                attach_command: "tmux attach-session -t jade-main-29".into(),
                attempt: 1,
                status: SessionStatus::Completed,
                started_at_ms: 1,
                updated_at_ms: 2,
            },
        )
        .unwrap();

        let reconciliation = reconcile_pending_main_session(&config, &issue, &handoff, &state)
            .unwrap()
            .expect("expected completed session reconciliation");

        let MainSessionReconciliation::Terminal(result) = reconciliation else {
            panic!("expected terminal completed reconciliation");
        };
        assert!(result.success);
        assert!(!result.pending_session);
        assert_eq!(result.session_id.as_deref(), Some("jade-main-29"));
        assert!(result.message.contains("registry status completed"));
    }

    #[test]
    fn main_loop_keeps_missing_pending_session_registry_active_instead_of_relaunching() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = main_loop_test_config();
        config.artifacts.root = temp.path().join("artifacts");
        let issue = tracker_issue("In Progress");
        let claim = test_claim(&issue);
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let mut state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
        state.last_event = Some("SessionRunning".into());
        state.backend_session_id = Some("jade-main-missing".into());

        let reconciliation = reconcile_pending_main_session(&config, &issue, &handoff, &state)
            .unwrap()
            .expect("expected active missing-registry reconciliation");

        let MainSessionReconciliation::Active {
            status,
            source,
            evidence,
        } = reconciliation
        else {
            panic!("expected active reconciliation");
        };
        assert_eq!(status, "unknown");
        assert_eq!(source, "runtime");
        assert!(evidence.contains("missing from session registry"));
    }

    #[test]
    fn run_loop_handoff_plan_uses_issue_workspace_and_branch_plan() {
        let config = test_config();
        let issue = tracker_issue("In Progress");

        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();

        assert_eq!(
            handoff.workspace_key,
            "issue-29-wire-runtime-state-persistence-into-main-loop"
        );
        assert!(handoff
            .workspace_path
            .ends_with("issue-29-wire-runtime-state-persistence-into-main-loop"));
        assert_eq!(
            handoff.branch_name,
            "feature/issue-29-wire-runtime-state-persistence-into-main-loop"
        );
        assert_eq!(
            handoff.pull_request.title,
            "#29: Wire runtime state persistence into main loop"
        );
        assert_eq!(handoff.pull_request.base_branch, "main");
    }

    #[test]
    fn run_loop_handoff_plan_rejects_branch_for_different_issue() {
        let config = test_config();
        let mut issue = tracker_issue("In Progress");
        issue.branch_name = Some("feature/issue-99-other-work".into());

        let error = run_loop_handoff_plan(&config, &issue).unwrap_err();

        assert!(matches!(
            error,
            HandoffError::BranchIssueMismatch {
                expected_issue,
                found_issue,
                ..
            } if expected_issue == "29" && found_issue == "99"
        ));
    }

    #[test]
    fn run_loop_handoff_workpad_records_planned_pr_evidence() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let result = IssueExecutionResult {
            workspace_path: handoff.workspace_path.clone(),
            backend: "dry-run".into(),
            profile_id: None,
            instance_name: None,
            success: true,
            pending_session: false,
            session_id: Some("session-33".into()),
            run_id: None,
            backend_log_path: None,
            backend_attach_command: None,
            message: "ok".into(),
            usage_limit_pause: None,
            prompt_artifact_path: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
                author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
                applied_keys: vec!["user.name".into(), "user.email".into()],
            },
            live_handoff: Some(RunLoopLiveHandoff {
                worktree: LiveWorktreeResult {
                    workspace_path: handoff.workspace_path.clone(),
                    branch_name: handoff.branch_name.clone(),
                    created: true,
                },
                publication: PullRequestPublication {
                    branch_pushed: true,
                    pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                    pr_created: true,
                },
                verification: "skipped:not_configured".into(),
                project_pr_link_verified: Some(true),
                pull_request_ready: Some(PullRequestReadyStatus {
                    pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                    was_draft: false,
                    marked_ready: false,
                }),
            }),
            handoff_verification: Some("skipped:not_configured".into()),
        };

        let workpad = run_loop_handoff_workpad(&issue, &result, &handoff, None);

        assert!(workpad.contains("### Plan"));
        assert!(workpad.contains("### Work Log"));
        assert!(workpad.contains("- [x] Read the issue contract"));
        assert!(workpad.contains("### Planned Handoff"));
        assert!(workpad.contains("Actor role: `implementation_agent`"));
        assert!(
            workpad.contains("Git identity: `applied:Jade Symphony Agent <jade@example.invalid>`")
        );
        assert!(workpad
            .contains("Workspace key: `issue-29-wire-runtime-state-persistence-into-main-loop`"));
        assert!(workpad
            .contains("Branch: `feature/issue-29-wire-runtime-state-persistence-into-main-loop`"));
        assert!(workpad.contains("PR title: `#29: Wire runtime state persistence into main loop`"));
        assert!(workpad.contains("Handoff verification: `skipped:not_configured`"));
        assert!(workpad.contains("Live PR: `https://github.com/Alive24/jade-symphony/pull/45`"));
    }

    #[test]
    fn live_run_loop_handoff_records_pr_link_through_tracker() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let mut result = successful_live_handoff_result(&handoff);
        let adapter = RecordingAdapter::default();

        assert!(apply_live_handoff_pr_link(
            &adapter,
            &issue.identifier,
            &mut result
        ));

        assert!(result.success);
        assert_eq!(
            adapter.operations(),
            vec!["link_pr:#29:https://github.com/Alive24/jade-symphony/pull/45"]
        );
    }

    #[test]
    fn live_run_loop_handoff_skips_link_comment_when_pr_already_visible() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let mut result = successful_live_handoff_result(&handoff);
        let adapter = RecordingAdapter::default();
        adapter
            .linked_pull_requests
            .borrow_mut()
            .push(jade_symphony::model::LinkedPullRequest {
                number: Some(45),
                url: Some("https://github.com/Alive24/jade-symphony/pull/45".into()),
                state: Some("OPEN".into()),
                is_draft: Some(false),
                ..Default::default()
            });

        assert!(apply_live_handoff_pr_link(
            &adapter,
            &issue.identifier,
            &mut result
        ));

        assert!(result.success);
        assert!(adapter.operations().is_empty());
    }

    #[test]
    fn handoff_verification_skips_when_not_configured() {
        let config = test_config();
        let temp = tempfile::tempdir().unwrap();

        let verification = run_handoff_verification(temp.path(), &config);

        assert!(verification.success);
        assert_eq!(verification.summary, "skipped:not_configured");
    }

    #[test]
    fn handoff_verification_runs_configured_commands() {
        let mut config = test_config();
        config.verification.commands = vec!["printf verified > verification.txt".into()];
        config.verification.timeout_ms = 5_000;
        let temp = tempfile::tempdir().unwrap();

        let verification = run_handoff_verification(temp.path(), &config);

        assert!(verification.success);
        assert_eq!(verification.summary, "passed:1 command(s)");
        assert_eq!(
            std::fs::read_to_string(temp.path().join("verification.txt")).unwrap(),
            "verified"
        );
    }

    #[test]
    fn live_run_loop_handoff_link_failure_blocks_agent_review() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let mut result = successful_live_handoff_result(&handoff);
        let adapter = RecordingAdapter {
            fail_link_pr: true,
            ..Default::default()
        };

        assert!(!apply_live_handoff_pr_link(
            &adapter,
            &issue.identifier,
            &mut result
        ));

        assert!(!result.success);
        assert!(result.message.contains("handoff PR link repair failed"));
        assert_eq!(
            result
                .live_handoff
                .as_ref()
                .and_then(|handoff| handoff.project_pr_link_verified),
            Some(false)
        );
    }

    #[test]
    fn live_run_loop_handoff_requires_verified_project_pr_linkage() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let mut result = successful_live_handoff_result(&handoff);
        let adapter = RecordingAdapter {
            confirm_link_pr: false,
            ..Default::default()
        };

        assert!(!apply_live_handoff_pr_link(
            &adapter,
            &issue.identifier,
            &mut result
        ));

        assert!(!result.success);
        assert!(result.message.contains("not Project-visible"));
        assert_eq!(
            result
                .live_handoff
                .as_ref()
                .and_then(|handoff| handoff.project_pr_link_verified),
            Some(false)
        );
    }

    fn successful_live_handoff_result(handoff: &IssueHandoffPlan) -> IssueExecutionResult {
        IssueExecutionResult {
            workspace_path: handoff.workspace_path.clone(),
            backend: "dry-run".into(),
            profile_id: None,
            instance_name: None,
            success: true,
            pending_session: false,
            session_id: Some("session-33".into()),
            run_id: None,
            backend_log_path: None,
            backend_attach_command: None,
            message: "ok".into(),
            usage_limit_pause: None,
            prompt_artifact_path: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
                author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
                applied_keys: vec!["user.name".into(), "user.email".into()],
            },
            live_handoff: Some(RunLoopLiveHandoff {
                worktree: LiveWorktreeResult {
                    workspace_path: handoff.workspace_path.clone(),
                    branch_name: handoff.branch_name.clone(),
                    created: true,
                },
                publication: PullRequestPublication {
                    branch_pushed: true,
                    pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                    pr_created: true,
                },
                verification: "skipped:not_configured".into(),
                project_pr_link_verified: Some(true),
                pull_request_ready: Some(PullRequestReadyStatus {
                    pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                    was_draft: false,
                    marked_ready: false,
                }),
            }),
            handoff_verification: Some("skipped:not_configured".into()),
        }
    }

    #[test]
    fn handoff_verification_failure_blocks_success() {
        let mut config = test_config();
        config.verification.commands = vec!["echo nope >&2; exit 7".into()];
        config.verification.timeout_ms = 5_000;
        let temp = tempfile::tempdir().unwrap();

        let verification = run_handoff_verification(temp.path(), &config);

        assert!(!verification.success);
        assert!(verification.summary.contains("failed command=`echo nope"));
        assert!(verification.summary.contains("status 7"));
    }

    #[test]
    fn usage_limit_pause_workpad_preserves_tracker_state_boundary() {
        let issue = tracker_issue("In Progress");
        let result = IssueExecutionResult {
            workspace_path: PathBuf::from("/tmp/jade/issue-63"),
            backend: "codex".into(),
            profile_id: None,
            instance_name: None,
            success: false,
            pending_session: false,
            session_id: Some("session-63".into()),
            run_id: None,
            backend_log_path: None,
            backend_attach_command: None,
            message: "Codex subprocess exited with status 1".into(),
            usage_limit_pause: Some(UsageLimitPause {
                classifier: "usage_limit".into(),
                evidence: "usage limit reached".into(),
            }),
            prompt_artifact_path: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::NotGitRepository,
                author: None,
                applied_keys: Vec::new(),
            },
            live_handoff: None,
            handoff_verification: None,
        };
        let pause = result.usage_limit_pause.as_ref().unwrap();
        let workpad = run_loop_usage_limit_pause_workpad(&issue, &result, pause, 20_000);

        assert!(workpad.contains("### Usage-Limit Pause"));
        assert!(workpad.contains("Classifier: `usage_limit`"));
        assert!(workpad.contains("Tracker state was not advanced to `Agent Review`"));
        assert!(workpad.contains("Retry backoff: `20000ms`"));
    }

    #[test]
    fn rework_transition_writes_diagnostic_before_state_change() {
        let adapter = RecordingAdapter::default();
        let issue = tracker_issue("Agent Review");
        let diagnostic = ReworkDiagnostic::validation_failure(
            issue.identifier.clone(),
            "cargo test",
            "failing test output",
        );

        let config = test_config();
        transition_issue_to_rework_with_diagnostic(&config, &adapter, &issue, &diagnostic).unwrap();

        assert_eq!(
            adapter.operations(),
            vec![
                "comment:#29".to_string(),
                "set_state:#29:rework".to_string()
            ]
        );
    }

    #[test]
    fn rework_transition_does_not_set_state_when_timeline_comment_fails() {
        let adapter = RecordingAdapter {
            fail_comment: true,
            ..Default::default()
        };
        let issue = tracker_issue("Agent Review");
        let diagnostic = ReworkDiagnostic::validation_failure(
            issue.identifier.clone(),
            "cargo test",
            "failing test output",
        );

        let config = test_config();
        assert!(
            transition_issue_to_rework_with_diagnostic(&config, &adapter, &issue, &diagnostic)
                .is_err()
        );
        assert!(adapter.operations().is_empty());
    }

    #[test]
    fn merge_completion_closes_issue_after_workpad_and_done_state() {
        let adapter = RecordingAdapter::default();
        let issue = tracker_issue("Merging");
        let workpad = "## Jade Symphony Merge Run\n\n### Merge Action\n";

        let config = test_config();
        record_done_merge_lane_completion(&config, &adapter, &issue, workpad).unwrap();

        assert_eq!(
            adapter.operations(),
            vec![
                "comment:#29".to_string(),
                "set_state:#29:done".to_string(),
                "close_issue:#29".to_string()
            ]
        );
    }

    #[test]
    fn run_loop_agent_review_handoff_blocks_missing_pr_url() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let result = IssueExecutionResult {
            workspace_path: handoff.workspace_path.clone(),
            backend: "dry-run".into(),
            profile_id: None,
            instance_name: None,
            success: true,
            pending_session: false,
            session_id: Some("session-57".into()),
            run_id: None,
            backend_log_path: None,
            backend_attach_command: None,
            message: "ok".into(),
            usage_limit_pause: None,
            prompt_artifact_path: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
                author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
                applied_keys: vec!["user.name".into(), "user.email".into()],
            },
            live_handoff: None,
            handoff_verification: None,
        };

        let workpad = run_loop_handoff_workpad(&issue, &result, &handoff, None);
        let evidence =
            run_loop_agent_review_handoff_evidence(&issue, &result, &handoff, Some(&workpad));
        let report = evaluate_agent_review_handoff(&evidence);

        assert!(!report.is_ready());
        assert_eq!(report.target_state.as_deref(), Some("need_human_input"));
        assert!(evidence
            .no_pr_blocker
            .unwrap()
            .contains("No pull request URL"));
    }

    #[test]
    fn run_loop_agent_review_handoff_passes_with_pr_url() {
        let config = test_config();
        let mut issue = tracker_issue("In Progress");
        issue
            .linked_pull_requests
            .push(jade_symphony::model::LinkedPullRequest {
                id: Some("PR_57".into()),
                number: Some(57),
                url: Some("https://github.com/Alive24/jade-symphony/pull/57".into()),
                state: Some("OPEN".into()),
                is_draft: Some(false),
                ..Default::default()
            });
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let result = IssueExecutionResult {
            workspace_path: handoff.workspace_path.clone(),
            backend: "dry-run".into(),
            profile_id: None,
            instance_name: None,
            success: true,
            pending_session: false,
            session_id: Some("session-57".into()),
            run_id: None,
            backend_log_path: None,
            backend_attach_command: None,
            message: "ok".into(),
            usage_limit_pause: None,
            prompt_artifact_path: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
                author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
                applied_keys: vec!["user.name".into(), "user.email".into()],
            },
            live_handoff: None,
            handoff_verification: None,
        };

        let workpad = run_loop_handoff_workpad(&issue, &result, &handoff, None);
        let evidence =
            run_loop_agent_review_handoff_evidence(&issue, &result, &handoff, Some(&workpad));
        let report = evaluate_agent_review_handoff(&evidence);

        assert!(report.is_ready());
        assert_eq!(report.target_state.as_deref(), Some("agent_review"));
        assert_eq!(
            evidence.pull_request_url.as_deref(),
            Some("https://github.com/Alive24/jade-symphony/pull/57")
        );
    }

    #[test]
    fn run_loop_agent_review_handoff_blocks_draft_pr_and_missing_workpad_evidence() {
        let config = test_config();
        let mut issue = tracker_issue("In Progress");
        issue
            .linked_pull_requests
            .push(jade_symphony::model::LinkedPullRequest {
                id: Some("PR_57".into()),
                number: Some(57),
                url: Some("https://github.com/Alive24/jade-symphony/pull/57".into()),
                state: Some("OPEN".into()),
                is_draft: Some(false),
                ..Default::default()
            });
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let result = successful_live_handoff_result(&handoff);

        let missing_workpad_evidence =
            run_loop_agent_review_handoff_evidence(&issue, &result, &handoff, None);
        let missing_workpad_report = evaluate_agent_review_handoff(&missing_workpad_evidence);

        assert!(!missing_workpad_report.is_ready());
        assert!(missing_workpad_report
            .missing
            .contains(&"Main Workpad `### Plan`".into()));
        assert!(missing_workpad_report
            .missing
            .contains(&"Main Workpad `### Work Log`".into()));

        let mut draft_result = successful_live_handoff_result(&handoff);
        if let Some(live_handoff) = draft_result.live_handoff.as_mut() {
            live_handoff.pull_request_ready = Some(PullRequestReadyStatus {
                pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                was_draft: true,
                marked_ready: false,
            });
        }
        let draft_workpad = run_loop_handoff_workpad(&issue, &draft_result, &handoff, None);
        let draft_evidence = run_loop_agent_review_handoff_evidence(
            &issue,
            &draft_result,
            &handoff,
            Some(&draft_workpad),
        );
        let draft_report = evaluate_agent_review_handoff(&draft_evidence);

        assert!(!draft_report.is_ready());
        assert!(draft_report
            .missing
            .contains(&"non-draft pull request".into()));
    }

    #[test]
    fn parses_forge_create_flags() {
        let command = Command::parse(vec![
            "forge".into(),
            "create".into(),
            "--workflow".into(),
            "examples/dry-run-workflow.md".into(),
            "--title".into(),
            "Create issue".into(),
            "--body".into(),
            forge_contract(),
            "--status".into(),
            "todo".into(),
            "--project".into(),
            "workflow".into(),
            "--project-field".into(),
            "Capability=CLI".into(),
            "--assignee".into(),
            "@Alive24".into(),
            "--write".into(),
        ])
        .unwrap();

        let Command::ForgeCreate {
            workflow_path,
            title,
            markdown,
            status,
            project,
            project_fields,
            assignees,
            write,
            dry_run,
        } = command
        else {
            panic!("expected forge create command");
        };

        assert_eq!(workflow_path, PathBuf::from("examples/dry-run-workflow.md"));
        assert_eq!(title, "Create issue");
        assert!(markdown.contains("## Issue Goal"));
        assert_eq!(status, ForgeStatusArg::Todo);
        assert_eq!(project.as_deref(), Some("workflow"));
        assert_eq!(
            project_fields,
            vec![ProjectFieldAssignment {
                name: "Capability".into(),
                value: "CLI".into()
            }]
        );
        assert_eq!(assignees, vec!["@Alive24".to_string()]);
        assert!(write);
        assert!(!dry_run);
    }

    #[test]
    fn parses_forge_promote_flags() {
        let command = Command::parse(vec![
            "forge".into(),
            "promote".into(),
            "#241".into(),
            "--workflow".into(),
            "examples/dry-run-workflow.md".into(),
            "--title".into(),
            "Promoted issue".into(),
            "--body".into(),
            forge_contract(),
            "--operator-confirmation".into(),
            "promote it".into(),
            "--decision".into(),
            "Keep this as an in-place promotion.".into(),
            "--scope-change".into(),
            "Promoted body is now executable.".into(),
            "--dependency-context".into(),
            "Dependencies: none.".into(),
            "--readback-summary".into(),
            "Operator confirmed the dry-run preview before write.".into(),
            "--dry-run".into(),
        ])
        .unwrap();

        let Command::ForgePromote {
            workflow_path,
            issue_ref,
            title,
            markdown,
            promotion_note,
            write,
            dry_run,
        } = command
        else {
            panic!("expected forge promote command");
        };

        assert_eq!(workflow_path, PathBuf::from("examples/dry-run-workflow.md"));
        assert_eq!(issue_ref, "#241");
        assert_eq!(title, "Promoted issue");
        assert!(markdown.contains("## Issue Goal"));
        assert_eq!(promotion_note.operator_confirmation, "promote it");
        assert_eq!(
            promotion_note.decisions,
            vec!["Keep this as an in-place promotion.".to_string()]
        );
        assert_eq!(
            promotion_note.readback_summaries,
            vec!["Operator confirmed the dry-run preview before write.".to_string()]
        );
        assert!(!write);
        assert!(dry_run);
    }

    #[test]
    fn parses_forge_rework_flags() {
        let temp = tempfile::tempdir().unwrap();
        let body_path = temp.path().join("body.md");
        let evidence_path = temp.path().join("evidence.md");
        std::fs::write(&body_path, forge_contract()).unwrap();
        std::fs::write(&evidence_path, "Reviewer changed the execution contract.").unwrap();

        let command = Command::parse(vec![
            "forge".into(),
            "rework".into(),
            "#282".into(),
            "--workflow".into(),
            "examples/dry-run-workflow.md".into(),
            "--title".into(),
            "Reworked contract".into(),
            "--body-file".into(),
            body_path.display().to_string(),
            "--evidence-file".into(),
            evidence_path.display().to_string(),
            "--operator-confirmation".into(),
            "send it back to Rework".into(),
            "--dry-run".into(),
        ])
        .unwrap();

        let Command::ForgeRework { options } = command else {
            panic!("expected forge rework command");
        };

        assert_eq!(
            options.workflow_path,
            PathBuf::from("examples/dry-run-workflow.md")
        );
        assert_eq!(options.issue_ref, "#282");
        assert_eq!(options.title, "Reworked contract");
        assert!(options.markdown.contains("## Issue Goal"));
        assert_eq!(options.evidence, "Reviewer changed the execution contract.");
        assert_eq!(options.operator_confirmation, "send it back to Rework");
        assert!(!options.write);
        assert!(options.dry_run);
    }

    #[test]
    fn forge_rework_writes_content_then_evidence_then_status() {
        let config = test_config();
        let adapter = RecordingAdapter::default();
        let mut issue = tracker_issue_with_ref("#282", "Old reviewed contract", "Human Review");
        issue.description = Some(forge_contract());
        let done_main_claim = LaneClaim::active(
            "#282",
            LaneClaimLane::Main,
            LaneClaimActor::Codex,
            LaneClaimSource::Manual,
            1_779_000_900_123,
        )
        .with_state(LaneClaimState::Done);
        issue.project_fields.insert(
            "Main Agent".into(),
            serde_json::Value::String(done_main_claim.render()),
        );
        adapter
            .issues
            .borrow_mut()
            .insert(issue.identifier.clone(), issue);

        forge_rework_with_adapter(
            &config,
            &adapter,
            ForgeReworkInput {
                issue_ref: "#282".into(),
                title: "Reworked contract".into(),
                markdown: forge_contract(),
                evidence: "Prior Human Review evidence is superseded by the revised contract."
                    .into(),
                operator_confirmation: "route to Rework".into(),
                dry_run: false,
            },
        )
        .unwrap();

        assert_eq!(
            adapter.operations(),
            vec![
                "update_issue_content:#282".to_string(),
                "comment:#282".to_string(),
                "set_state:#282:rework".to_string(),
            ]
        );
        assert_eq!(
            adapter
                .get_issue("#282")
                .unwrap()
                .unwrap()
                .normalized_state(),
            "rework"
        );
    }

    #[test]
    fn forge_rework_records_diagnostic_for_active_human_review_claims() {
        let config = test_config();
        let adapter = RecordingAdapter::default();
        let mut issue = tracker_issue_with_ref("#282", "Reviewed contract", "Human Review");
        issue.description = Some(forge_contract());
        let active_review_claim = LaneClaim::active(
            "#282",
            LaneClaimLane::Review,
            LaneClaimActor::Gemini,
            LaneClaimSource::Manual,
            1_779_000_900_123,
        );
        issue.project_fields.insert(
            "Review Agent".into(),
            serde_json::Value::String(active_review_claim.render()),
        );
        adapter
            .issues
            .borrow_mut()
            .insert(issue.identifier.clone(), issue);

        let error = forge_rework_with_adapter(
            &config,
            &adapter,
            ForgeReworkInput {
                issue_ref: "#282".into(),
                title: "Reworked contract".into(),
                markdown: forge_contract(),
                evidence: "Reviewer changed the contract.".into(),
                operator_confirmation: "route to Rework".into(),
                dry_run: false,
            },
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("active Review Agent claim"));
        assert_eq!(adapter.operations(), vec!["comment:#282".to_string()]);
    }

    #[test]
    fn manual_main_claim_accepts_rework() {
        let config = test_config();
        let issue = tracker_issue("Rework");

        validate_lane_claim_state(&issue, AgentSessionLaneArg::Main, &config).unwrap();
    }

    #[test]
    fn manual_main_claim_rejects_parent_with_incomplete_native_subissues() {
        let config = test_config();
        let mut issue = tracker_issue("Todo");
        issue.project_fields.insert(
            "GitHub Native Subissues".into(),
            serde_json::json!([
                {"identifier": "#272", "project_state": "Done"},
                {"identifier": "#273", "project_state": "Agent Review"}
            ]),
        );

        let error = validate_lane_claim_state(&issue, AgentSessionLaneArg::Main, &config)
            .unwrap_err()
            .to_string();

        assert!(error.contains("blocked by incomplete native subissues"));
        assert!(error.contains("#273=Agent Review"));
    }

    #[test]
    fn renders_strict_promotion_note_template() {
        let note = render_promotion_note(
            "#262",
            "Standardize Issue Forge Reflect promotion notes",
            &PromotionNoteInput {
                operator_confirmation: "promote it".into(),
                decisions: vec!["Use the CLI as the enforcement point.".into()],
                scope_changes: vec!["The Backlog seed became an executable Todo issue.".into()],
                dependencies_context: vec![
                    "Dependencies: none; related context is non-blocking.".into()
                ],
                readback_summaries: vec![
                    "Operator confirmed the dry-run preview matched the promotion intent.".into(),
                ],
            },
            &["Readback confirmed issue `#262` and Project status `Todo`.".into()],
        );

        assert!(note.contains("## Promotion Note"));
        assert!(note.contains("- Source Backlog issue: #262"));
        assert!(note.contains("- Operator confirmation: \"promote it\""));
        assert!(note.contains("## Key Operator Decisions"));
        assert!(note.contains("## Major Scope Changes From Seed"));
        assert!(note.contains("## Dependencies and Context"));
        assert!(note.contains("## Verification Readback"));
        assert!(note.contains("- Readback confirmed issue `#262` and Project status `Todo`."));
        assert!(
            note.contains("- Operator confirmed the dry-run preview matched the promotion intent.")
        );
    }

    #[test]
    fn parses_link_pr_flags() {
        let command = Command::parse(vec![
            "project".into(),
            "link-pr".into(),
            "examples/github-project-workflow.md".into(),
            "#127".into(),
            "https://github.com/Alive24/jade-symphony/pull/128".into(),
            "--write".into(),
        ])
        .unwrap();

        let Command::LinkPr {
            workflow_path,
            issue_ref,
            pr_ref,
            write,
        } = command
        else {
            panic!("expected link-pr command");
        };

        assert_eq!(
            workflow_path,
            PathBuf::from("examples/github-project-workflow.md")
        );
        assert_eq!(issue_ref, "#127");
        assert_eq!(pr_ref, "https://github.com/Alive24/jade-symphony/pull/128");
        assert!(write);
    }

    #[test]
    fn link_pr_helper_respects_write_intent() {
        let adapter = RecordingAdapter::default();

        assert!(!link_pr_with_adapter(&adapter, "#127", "PR_128", false).unwrap());
        assert!(adapter.operations().is_empty());

        assert!(link_pr_with_adapter(&adapter, "#127", "PR_128", true).unwrap());
        assert_eq!(adapter.operations(), vec!["link_pr:#127:PR_128"]);
    }

    #[test]
    fn link_pr_helper_skips_repair_when_project_readback_already_has_pr() {
        let adapter = RecordingAdapter::default();
        adapter
            .linked_pull_requests
            .borrow_mut()
            .push(jade_symphony::model::LinkedPullRequest {
                number: Some(128),
                url: Some("https://github.com/Alive24/jade-symphony/pull/128".into()),
                state: Some("OPEN".into()),
                is_draft: Some(false),
                ..Default::default()
            });

        assert!(!link_pr_with_adapter(&adapter, "#127", "PR_128", true).unwrap());
        assert!(adapter.operations().is_empty());
    }

    #[test]
    fn parses_forge_validate_issue_flags() {
        let command = Command::parse(vec![
            "forge".into(),
            "validate".into(),
            "--workflow".into(),
            "examples/github-project-workflow.md".into(),
            "--issue".into(),
            "#248".into(),
            "--status".into(),
            "todo".into(),
        ])
        .unwrap();

        let Command::ForgeValidate {
            workflow_path,
            status,
            title,
            markdown,
            issue_ref,
        } = command
        else {
            panic!("expected forge validate command");
        };

        assert_eq!(
            workflow_path,
            PathBuf::from("examples/github-project-workflow.md")
        );
        assert_eq!(status, Some(ForgeStatusArg::Todo));
        assert!(title.is_empty());
        assert!(markdown.is_empty());
        assert_eq!(issue_ref.as_deref(), Some("#248"));
    }

    #[test]
    fn parses_forge_validate_issue_with_candidate_body_flags() {
        let temp = tempfile::tempdir().unwrap();
        let body_path = temp.path().join("candidate.md");
        std::fs::write(&body_path, forge_contract()).unwrap();

        let command = Command::parse(vec![
            "forge".into(),
            "validate".into(),
            "--workflow".into(),
            "examples/github-project-workflow.md".into(),
            "--issue".into(),
            "#293".into(),
            "--status".into(),
            "todo".into(),
            "--title".into(),
            "Candidate promoted title".into(),
            "--body-file".into(),
            body_path.display().to_string(),
        ])
        .unwrap();

        let Command::ForgeValidate {
            status,
            title,
            markdown,
            issue_ref,
            ..
        } = command
        else {
            panic!("expected forge validate command");
        };

        assert_eq!(status, Some(ForgeStatusArg::Todo));
        assert_eq!(title, "Candidate promoted title");
        assert!(markdown.contains("## Issue Goal"));
        assert_eq!(issue_ref.as_deref(), Some("#293"));
    }

    #[test]
    fn rejects_removed_flat_forge_commands() {
        let error = Command::parse(vec![
            "forge-create".into(),
            "--workflow".into(),
            "workflows/jade-symphony.md".into(),
        ])
        .unwrap_err();

        assert!(error.contains("Usage:"));
    }

    #[test]
    fn rejects_forge_create_with_both_body_and_file() {
        let error = Command::parse(vec![
            "forge".into(),
            "create".into(),
            "--workflow".into(),
            "WORKFLOW.md".into(),
            "--title".into(),
            "Create issue".into(),
            "--body".into(),
            forge_contract(),
            "--body-file".into(),
            "issue.md".into(),
        ])
        .unwrap_err();

        assert!(error.contains("Usage:"));
    }

    #[test]
    fn validates_forge_create_contract_before_tracker_write() {
        let config = test_config();
        assert!(
            validate_forge_create_contract("Create issue", &forge_contract(), &config, &[]).is_ok()
        );

        let error = validate_forge_create_contract("Thin issue", "make it better", &config, &[])
            .unwrap_err();
        assert!(error.contains("tracker issue was not created"));
    }

    #[test]
    fn forge_create_draft_validation_uses_intended_assignee_for_live_github() {
        let config = live_github_config(false);
        let assignees = vec!["Alive24".to_string()];

        let report = validate_forge_create_report_with_assignees(
            "Create issue",
            &forge_contract(),
            &config,
            &assignees,
        )
        .unwrap();

        assert!(report.decision.is_dispatchable());
    }

    #[test]
    fn forge_validate_candidate_context_uses_live_issue_assignee() {
        let config = live_github_config(false);
        let assignees = vec!["Alive24".to_string()];
        let report = forge_validation_report(
            ForgeStatusArg::Todo,
            "Candidate promoted title",
            &forge_contract(),
            &config,
            &assignees,
        )
        .unwrap();
        let categories = forge_missing_categories(&report);

        assert!(report.decision.is_dispatchable());
        assert!(categories.candidate_missing.is_empty());
        assert!(categories.live_context_missing.is_empty());
    }

    #[test]
    fn forge_validate_candidate_context_reports_unassigned_live_issue() {
        let config = live_github_config(false);
        let report = forge_validation_report(
            ForgeStatusArg::Todo,
            "Candidate promoted title",
            &forge_contract(),
            &config,
            &[],
        )
        .unwrap();
        let categories = forge_missing_categories(&report);

        assert_eq!(
            categories.live_context_missing,
            vec!["live GitHub issue assignee".to_string()]
        );
        assert!(categories.candidate_missing.is_empty());
    }

    #[test]
    fn forge_validate_candidate_context_reports_candidate_gaps_separately() {
        let config = live_github_config(false);
        let assignees = vec!["Alive24".to_string()];
        let report = forge_validation_report(
            ForgeStatusArg::Todo,
            "Thin issue",
            "make forge better",
            &config,
            &assignees,
        )
        .unwrap();
        let categories = forge_missing_categories(&report);

        assert!(!categories.candidate_missing.is_empty());
        assert!(categories.live_context_missing.is_empty());
    }

    #[test]
    fn forge_create_live_github_requires_assignee_before_creation() {
        let config = live_github_config(false);

        let error = validate_forge_create_contract("Create issue", &forge_contract(), &config, &[])
            .unwrap_err();

        assert!(error.contains("tracker issue was not created"));
        assert!(forge_create_requires_assignee(
            &config,
            ForgeStatusArg::Todo
        ));
        assert!(!forge_create_requires_assignee(
            &config,
            ForgeStatusArg::Backlog
        ));
    }

    #[test]
    fn forge_create_entrypoint_rejects_live_github_without_assignee() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_path = temp.path().join("WORKFLOW.md");
        std::fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\n  assignee_filter:\n    allow_unassigned: false\nobservability:\n  logs_root: log\n---\nPrompt",
        )
        .unwrap();

        let error = forge_create(ForgeCreateOptions {
            workflow_path,
            title: "Create issue".into(),
            markdown: forge_contract(),
            status: ForgeStatusArg::Todo,
            project: None,
            project_fields: Vec::new(),
            assignees: Vec::new(),
            write: true,
            dry_run: false,
        })
        .unwrap_err()
        .to_string();

        assert_eq!(
            error,
            "forge create --status Todo requires --assignee for live GitHub issue creation"
        );
    }

    #[test]
    fn forge_create_duplicate_title_match_normalizes_case_and_spacing() {
        let mut issue = tracker_issue("Todo");
        issue.identifier = "#143".into();
        issue.title = "Guard Issue Forge against duplicate tracker titles".into();
        let issues = [issue];

        let duplicate = find_duplicate_issue_title(
            &issues,
            "  guard   issue forge AGAINST duplicate tracker titles  ",
        )
        .unwrap();

        assert_eq!(duplicate.identifier, "#143");
    }

    #[test]
    fn forge_create_blocks_duplicate_tracker_title_before_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let fixture_path = temp.path().join("issues.json");
        let workflow_path = temp.path().join("WORKFLOW.md");
        let mut existing = tracker_issue("Todo");
        existing.identifier = "#143".into();
        existing.title = "Create issue".into();
        existing.url = Some("https://github.com/Alive24/jade-symphony/issues/143".into());
        std::fs::write(
            &fixture_path,
            serde_json::to_string(&vec![existing]).unwrap(),
        )
        .unwrap();
        std::fs::write(
            &workflow_path,
            format!(
                "---\ntracker:\n  kind: memory\n  fixture_path: {}\nobservability:\n  logs_root: log\n---\nPrompt",
                fixture_path.display()
            ),
        )
        .unwrap();

        let error = forge_create(ForgeCreateOptions {
            workflow_path,
            title: "Create issue".into(),
            markdown: forge_contract(),
            status: ForgeStatusArg::Todo,
            project: None,
            project_fields: Vec::new(),
            assignees: Vec::new(),
            write: true,
            dry_run: false,
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("duplicate tracker issue title detected"));
        assert!(error.contains("#143"));
        assert!(error.contains("https://github.com/Alive24/jade-symphony/issues/143"));
    }

    #[test]
    fn forge_create_can_use_memory_tracker_adapter() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_path = temp.path().join("WORKFLOW.md");
        std::fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: memory\nobservability:\n  logs_root: log\n---\nPrompt",
        )
        .unwrap();

        forge_create(ForgeCreateOptions {
            workflow_path,
            title: "Create issue".into(),
            markdown: forge_contract(),
            status: ForgeStatusArg::Todo,
            project: None,
            project_fields: Vec::new(),
            assignees: Vec::new(),
            write: true,
            dry_run: false,
        })
        .unwrap();
    }

    #[test]
    fn forge_create_write_initializes_backlog_without_status_transition() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.observability.logs_root = temp.path().join("logs");
        let adapter = RecordingAdapter::default();

        let create_result = write_forge_created_issue(
            &config,
            &adapter,
            ForgeCreateWriteInput {
                title: "Create Backlog seed".into(),
                markdown: forge_contract(),
                assignees: Vec::new(),
                status: ForgeStatusArg::Backlog,
                project_label: "test project",
                project_fields: &[],
            },
        )
        .unwrap();

        assert_eq!(create_result.issue_id, "dry-run:Create Backlog seed");
        assert_eq!(
            adapter.operations(),
            vec![
                "create_issue:dry-run:Create Backlog seed".to_string(),
                "add_project:dry-run:Create Backlog seed:backlog".to_string(),
            ]
        );
        assert_eq!(
            adapter
                .get_issue(&create_result.issue_id)
                .unwrap()
                .unwrap()
                .normalized_state(),
            "backlog"
        );
    }

    #[test]
    fn forge_create_success_reports_readback_metadata_when_available() {
        let mut issue = tracker_issue_with_ref("#305", "Created issue", "Backlog");
        issue.id = "I_kwDOSZP6c88AAAABC".into();
        issue.url = Some("https://github.com/Alive24/jade-symphony/issues/305".into());
        issue
            .project_fields
            .insert("Status".into(), "Backlog".into());

        let output = render_forge_create_success(
            &ForgeCreateResult {
                issue_id: issue.id.clone(),
                readback: Some(issue),
            },
            ForgeStatusArg::Backlog,
            0,
        );

        assert_eq!(
            output,
            "forge_create=ok issue_id=I_kwDOSZP6c88AAAABC issue=#305 url=https://github.com/Alive24/jade-symphony/issues/305 status=Backlog project_status=Backlog project_fields=0"
        );
    }

    #[test]
    fn forge_create_success_omits_unavailable_issue_metadata() {
        let output = render_forge_create_success(
            &ForgeCreateResult {
                issue_id: "memory:Create issue".into(),
                readback: None,
            },
            ForgeStatusArg::Todo,
            2,
        );

        assert_eq!(
            output,
            "forge_create=ok issue_id=memory:Create issue status=Todo project_fields=2"
        );
    }

    #[test]
    fn forge_create_readback_failure_reports_known_issue_location() {
        let adapter = RecordingAdapter::default();
        let mut issue = tracker_issue_with_ref("#305", "Created issue", "Need to Clarify");
        issue.id = "I_kwDOSZP6c88AAAABC".into();
        issue.url = Some("https://github.com/Alive24/jade-symphony/issues/305".into());
        adapter
            .issues
            .borrow_mut()
            .insert(issue.id.clone(), issue.clone());

        let error = verify_forge_created_issue_status(&adapter, &issue.id, ForgeStatusArg::Backlog)
            .unwrap_err()
            .to_string();

        assert!(error.contains("issue_id=I_kwDOSZP6c88AAAABC"));
        assert!(error.contains("issue=#305"));
        assert!(error.contains("url=https://github.com/Alive24/jade-symphony/issues/305"));
    }

    #[test]
    fn no_dispatch_sleeps_without_iteration_limit() {
        let options = RunLoopOptions {
            workflow_path: PathBuf::from("WORKFLOW.md"),
            max_iterations: None,
            once: false,
            max_concurrent: None,
            write: false,
            recover: false,
            display: DisplayMode::Plain,
        };

        assert_eq!(
            no_dispatch_action(options.iteration_limit(), 250),
            NoDispatchAction::SleepAndContinue { delay_ms: 250 }
        );
    }

    #[test]
    fn run_loop_write_mode_rejects_dry_run_backend_before_runtime_writes() {
        let temp = tempfile::tempdir().unwrap();
        let workspace_root = temp.path().join("workspaces");
        let logs_root = temp.path().join("logs");
        let workflow_path = temp.path().join("WORKFLOW.md");
        std::fs::write(
            &workflow_path,
            format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {}\nobservability:\n  logs_root: {}\nmain_lane:\n  backend: dry-run\n---\nPrompt",
                workspace_root.display(),
                logs_root.display()
            ),
        )
        .unwrap();

        let error = run_loop(RunLoopOptions {
            workflow_path: workflow_path.clone(),
            max_iterations: Some(1),
            once: false,
            max_concurrent: None,
            write: true,
            recover: false,
            display: DisplayMode::Plain,
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("write-mode main loop is blocked"));
        assert!(error.contains("main_lane.backend=dry-run"));
        assert!(error.contains(workflow_path.to_string_lossy().as_ref()));
        assert!(
            !workspace_root.exists(),
            "guard must fire before workspace creation"
        );
        assert!(!logs_root.exists(), "guard must fire before runtime writes");
    }

    #[test]
    fn run_loop_dry_run_preview_allows_dry_run_backend() {
        let temp = tempfile::tempdir().unwrap();
        let workspace_root = temp.path().join("workspaces");
        let logs_root = temp.path().join("logs");
        let workflow_path = temp.path().join("WORKFLOW.md");
        std::fs::write(
            &workflow_path,
            format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {}\nobservability:\n  logs_root: {}\nmain_lane:\n  backend: dry-run\n---\nPrompt",
                workspace_root.display(),
                logs_root.display()
            ),
        )
        .unwrap();

        run_loop(RunLoopOptions {
            workflow_path,
            max_iterations: Some(1),
            once: false,
            max_concurrent: None,
            write: false,
            recover: false,
            display: DisplayMode::Plain,
        })
        .unwrap();
    }

    #[test]
    fn no_dispatch_stops_for_bounded_write_loop() {
        let options = RunLoopOptions {
            workflow_path: PathBuf::from("WORKFLOW.md"),
            max_iterations: Some(2),
            once: false,
            max_concurrent: None,
            write: true,
            recover: false,
            display: DisplayMode::Plain,
        };

        assert_eq!(
            no_dispatch_action(options.iteration_limit(), 250),
            NoDispatchAction::Stop {
                reason: "no_dispatchable_issue"
            }
        );
    }

    #[test]
    fn no_dispatch_sleeps_for_unbounded_write_loop() {
        let options = RunLoopOptions {
            workflow_path: PathBuf::from("WORKFLOW.md"),
            max_iterations: None,
            once: false,
            max_concurrent: None,
            write: true,
            recover: false,
            display: DisplayMode::Plain,
        };

        assert_eq!(
            no_dispatch_action(options.iteration_limit(), 250),
            NoDispatchAction::SleepAndContinue { delay_ms: 250 }
        );
    }
}
