#[cfg(test)]
use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use cli::DisplayMode;
use cli::{CliLaneClaimSource, ForgeStatusArg};
use jade_symphony::agent::UsageLimitPause;
use jade_symphony::canonical_checkout::{
    canonical_checkout_refresh_status_line, canonical_checkout_status_line,
    canonical_checkout_warning_lines, inspect_canonical_checkout,
    refresh_canonical_checkout_before_write, CanonicalCheckoutRefreshMode,
};
use jade_symphony::config::RuntimeConfig;
#[cfg(test)]
use jade_symphony::doctor::ProjectAuditViolation;
#[cfg(test)]
use jade_symphony::doctor::{AuditSeverity, ProjectAuditReport};
use jade_symphony::event_log::{EventLog, EventRecord};
use jade_symphony::git_handoff::{
    commit_issue_worktree_changes, ensure_pull_request_ready, prepare_issue_worktree,
    publish_issue_pull_request, LiveWorktreeResult, ProcessHandoffCommandRunner,
    PullRequestPublication, PullRequestReadyStatus,
};
#[cfg(test)]
use jade_symphony::git_handoff::{CommandOutput, HandoffCommandRunner};
use jade_symphony::handoff::{
    evaluate_agent_review_handoff, plan_issue_handoff_for_profile,
    render_agent_review_handoff_workpad, AgentReviewHandoffEvidence, HandoffError,
    IssueHandoffPlan,
};
use jade_symphony::issue_workspace::{
    discover_issue_workspaces, infer_issue_ref_from_branch_or_path,
};
use jade_symphony::lane_claim::{
    LaneClaim, LaneClaimActor, LaneClaimLane, LaneClaimSource, LaneClaimState,
};
use jade_symphony::model::{
    normalize_state, GateDecision, LatestStatus, SessionStatusSnapshot, TrackerIssue,
};
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::ownership::{
    render_runtime_ownership_marker, runtime_ownership_decision, RuntimeOwnershipDecision,
    RuntimeOwnershipMarker,
};
use jade_symphony::profiles::selected_execution_profile;
use jade_symphony::progress::{run_with_progress_heartbeat, ProgressHeartbeatSpec};
use jade_symphony::prompt::render_prompt;
use jade_symphony::review::{
    transition_allowed_for_main_agent, FakeReviewOutcome, ReviewFreshnessInput,
};
#[cfg(test)]
use jade_symphony::review::{
    ReviewGateDecision, ReviewJob, ReviewJobState, ReviewOutcome, ReviewReworkClass,
    ReviewStaleReason,
};
#[cfg(test)]
use jade_symphony::rework::ReworkDiagnostic;
use jade_symphony::runtime_state::{
    load_runtime_states, mark_runtime_state_updated, record_runtime_retry,
    remove_runtime_state_for_issue, runtime_state_for_issue, upsert_runtime_state,
    RuntimeIssueState, RuntimeState, RuntimeTransition,
};
#[cfg(test)]
use jade_symphony::session_registry::save_session_record;
use jade_symphony::session_registry::{
    capture_tmux_pane_tail, classify_session_record, load_session_registry, read_log_tail,
    save_session_registry, session_registry_path, unix_timestamp_ms, AgentSessionRecord,
    SessionStatus,
};
use jade_symphony::skill_status::SkillStatusInput;
use jade_symphony::status_surface::{render_latest_status_bar, render_snapshot};
#[cfg(test)]
use jade_symphony::tracker::FollowUpIssueInput;
use jade_symphony::tracker::{
    adapter_from_config, claim_decision, ClaimDecision, ProjectFieldAssignment, TrackerAdapter,
    TrackerError,
};
use jade_symphony::workflow::WorkflowDefinition;
use jade_symphony::workspace::{run_workspace_command, GitIdentityApplyResult};

mod cli;
mod commands;
mod lanes;
mod orchestration;

use commands::autopilot::autopilot_plan;
#[cfg(test)]
use commands::autopilot::{
    build_autopilot_plan_from_parts, AutopilotCanonicalCheckout, AutopilotPlanInputs,
    AutopilotPlanSnapshot, AutopilotRuntimeSummary,
};
use commands::clean::{clean_audit_command, cleanup_plan_command};
use commands::debug::debug_report;
#[cfg(test)]
use commands::debug::{
    classify_dogfood_integration_gaps, is_controlled_dogfood_smoke_issue, session_status_summary,
};
use commands::doctor::{doctor, doctor_repair_human_review};
#[cfg(test)]
use commands::doctor::{doctor_health_label, hydrate_issues_for_doctor};
pub(crate) use commands::doctor::{DoctorAction, DoctorOptions, DoctorRepairIssueOptions};
use commands::follow_up::create_follow_up;
#[cfg(test)]
use commands::forge::{
    find_duplicate_issue_title, forge_create_requires_assignee, forge_missing_categories,
    forge_rework_with_adapter, forge_validation_report, issue_contract_assignees,
    render_forge_create_success, render_promotion_note, validate_forge_create_contract,
    validate_forge_create_report_with_assignees, verify_forge_created_issue_status,
    write_forge_created_issue, ForgeCreateResult, ForgeCreateWriteInput, ForgeReworkInput,
};
use commands::forge::{
    forge_create, forge_promote, forge_rework, forge_validate, ForgeCreateOptions,
};
pub(crate) use commands::forge::{ForgeReworkOptions, PromotionNoteInput};
#[cfg(test)]
use commands::gate::live_missing_assignee_gate_blocker;
pub(crate) use commands::gate::{
    evaluate_issue_for_current_source, gate_target_state, gate_workpad, quality_gate,
};
use commands::profiles::list_profiles;
#[cfg(test)]
use commands::project::filter_issues_by_state;
#[cfg(test)]
use commands::project::link_pr_with_adapter;
#[cfg(test)]
use commands::project::render_state_summary;
pub(crate) use commands::project::ProjectStateOptions;
use commands::project::{
    add_to_project, append_timeline_comment, link_pr, project_inspect, project_issue,
    project_state, set_state, upsert_workpad,
};
pub(crate) use commands::session::{
    agent_session_attach, agent_session_backend, agent_session_backend_spec, agent_session_list,
    agent_session_start, lane_claim_command, legacy_agent_session_start,
    record_agent_session_events, record_manual_lane_claim_evidence, render_parseable_lane_claim,
    rendered_lane_prompt_artifact_path, timeline_claim_actor, timeline_claim_run,
    timeline_pr_summary, AgentSessionLaneArg,
};
#[cfg(test)]
use commands::session::{
    lane_claim_for_manual_worker, matching_lane_claim_for_session, tmux_agent_command_for_lane,
    validate_lane_claim_state,
};
use commands::skills::skills_status;
#[cfg(test)]
use commands::status::render_plan_snapshot;
use commands::status::{plan, status_api};
use commands::workflow::{inspect, validate};
use commands::workspace::{
    cleanup_workspaces, workspace_adopt, workspace_ensure, workspace_list, workspace_show,
};
#[cfg(test)]
use commands::workspace::{
    ensure_inspection_worktree, validate_workspace_path_under_root, workspace_cleanup_plan,
    WorkspaceCleanupAction,
};
#[cfg(test)]
use lanes::claim::PoolClaimEligibility;
pub(crate) use lanes::claim::{
    pool_claim_eligibility, project_text_field, select_pool_worker_issues, worker_identity,
    WorkerLane,
};
pub(crate) use lanes::main_loop::{
    execute_issue_once, execute_issue_once_with_workspace_key, run_loop,
    runtime_state_issue_identifier, IssueExecutionResult, RunLoopOptions, RuntimeRecoveryCandidate,
};
#[cfg(test)]
use lanes::main_loop::{
    run_loop_resume_preflight, run_loop_resume_preflight_many, ResumePreflightAction,
};
pub(crate) use lanes::merge::MergeLoopOptions;
#[cfg(test)]
use lanes::merge::{
    finish_merge_agent_repaired_branch, merge_agent_reports_repaired,
    merge_agent_requests_human_input, merge_once_tick, record_done_merge_lane_completion,
    MergeOnceOutcome,
};
use lanes::merge::{merge_loop, merge_once};
#[cfg(test)]
use lanes::review::select_review_worker_issues;
#[cfg(test)]
use lanes::review::{
    apply_review_result, canonical_issue_body_without_workpad,
    check_review_verified_issue_body_checkboxes, render_automatic_review_prompt,
    render_manual_review_workpad, review_claim_for_issue, review_workspace_for_issue,
    terminal_review_claim_value, terminal_review_loop_claim_value,
    transition_issue_to_rework_with_diagnostic, validate_active_manual_review_claim,
    validate_manual_review_pass_claim,
};
use lanes::review::{
    review_claim, review_clear_claim, review_fake, review_freshness, review_loop,
    review_manual_pass, review_manual_reject, review_once, review_status,
};
pub(crate) use lanes::review::{ReviewLoopOptions, ReviewStatusCliOptions};
pub(crate) use orchestration::tracker_recovery::{
    add_timeline_comment_with_recovery, append_tracker_mutation_audit, close_issue_with_recovery,
    merge_completion_recovery_key, merge_decision_recovery_key, merge_pull_request_with_recovery,
    recovery_key, set_project_field_with_recovery, set_state_with_recovery, stable_recovery_hash,
    upsert_workpad_with_recovery, TrackerMutationAudit, TrackerMutationOutcome,
};
#[cfg(test)]
use orchestration::tracker_recovery::{issue_is_closed, tracker_recovery_marker};

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

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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
pub(crate) struct RunLoopLiveHandoff {
    pub(crate) worktree: LiveWorktreeResult,
    pub(crate) publication: PullRequestPublication,
    pub(crate) verification: String,
    pub(crate) project_pr_link_verified: Option<bool>,
    pub(crate) pull_request_ready: Option<PullRequestReadyStatus>,
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
