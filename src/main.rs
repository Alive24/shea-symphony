#[cfg(test)]
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use cli::Command;
#[cfg(test)]
use cli::DisplayMode;
#[cfg(test)]
use cli::{CliLaneClaimSource, ForgeStatusArg};
#[cfg(test)]
use jade_symphony::agent::UsageLimitPause;
use jade_symphony::config::RuntimeConfig;
#[cfg(test)]
use jade_symphony::doctor::ProjectAuditViolation;
#[cfg(test)]
use jade_symphony::doctor::{AuditSeverity, ProjectAuditReport};
#[cfg(test)]
use jade_symphony::event_log::EventLog;
#[cfg(test)]
use jade_symphony::git_handoff::{CommandOutput, HandoffCommandRunner};
#[cfg(test)]
use jade_symphony::git_handoff::{
    LiveWorktreeResult, PullRequestPublication, PullRequestReadyStatus,
};
#[cfg(test)]
use jade_symphony::handoff::evaluate_agent_review_handoff;
#[cfg(test)]
use jade_symphony::handoff::{plan_issue_handoff_for_profile, HandoffError, IssueHandoffPlan};
#[cfg(test)]
use jade_symphony::lane_claim::{
    LaneClaim, LaneClaimActor, LaneClaimLane, LaneClaimSource, LaneClaimState,
};
#[cfg(test)]
use jade_symphony::model::normalize_state;
#[cfg(test)]
use jade_symphony::model::SessionStatusSnapshot;
use jade_symphony::model::{LatestStatus, TrackerIssue};
#[cfg(test)]
use jade_symphony::orchestrator::Orchestrator;
#[cfg(test)]
use jade_symphony::ownership::render_runtime_ownership_marker;
#[cfg(test)]
use jade_symphony::ownership::{runtime_ownership_decision, RuntimeOwnershipDecision};
#[cfg(test)]
use jade_symphony::review::FakeReviewOutcome;
#[cfg(test)]
use jade_symphony::review::{
    ReviewGateDecision, ReviewJob, ReviewJobState, ReviewOutcome, ReviewReworkClass,
    ReviewStaleReason,
};
#[cfg(test)]
use jade_symphony::rework::ReworkDiagnostic;
#[cfg(test)]
use jade_symphony::runtime_state::{
    load_runtime_states, record_runtime_retry, runtime_state_for_issue, upsert_runtime_state,
};
#[cfg(test)]
use jade_symphony::runtime_state::{RuntimeIssueState, RuntimeState, RuntimeTransition};
#[cfg(test)]
use jade_symphony::session_registry::AgentSessionRecord;
#[cfg(test)]
use jade_symphony::session_registry::{load_session_registry, save_session_record};
#[cfg(test)]
use jade_symphony::session_registry::{
    save_session_registry, session_registry_path, SessionStatus,
};
#[cfg(test)]
use jade_symphony::skill_status::SkillStatusInput;
use jade_symphony::status_surface::render_latest_status_bar;
#[cfg(test)]
use jade_symphony::tracker::FollowUpIssueInput;
#[cfg(test)]
use jade_symphony::tracker::ProjectFieldAssignment;
#[cfg(test)]
use jade_symphony::tracker::TrackerAdapter;
#[cfg(test)]
use jade_symphony::workflow::WorkflowDefinition;
#[cfg(test)]
use jade_symphony::workspace::GitIdentityApplyResult;

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
    record_agent_session_events, record_manual_lane_claim_evidence,
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
    lane_claim_for_issue, pool_claim_eligibility, project_text_field, render_parseable_lane_claim,
    render_prompt_with_claim, select_pool_worker_issues, worker_identity, write_lane_claim_field,
    write_lane_claim_state, WorkerLane,
};
#[cfg(test)]
use lanes::main_loop::IssueExecutionResult;
#[cfg(test)]
use lanes::main_loop::{
    apply_live_handoff_pr_link, execute_issue_once_with_workspace_key,
    reconcile_pending_main_session, run_handoff_verification,
    run_loop_agent_review_handoff_evidence, run_loop_apply_recovery_handoff,
    run_loop_assignee_ownership_decision, run_loop_claim_action, run_loop_handoff_workpad,
    run_loop_ownership_workpad, run_loop_runtime_ownership, run_loop_runtime_state_for_issue,
    run_loop_runtime_state_with_result, run_loop_runtime_state_with_transition,
    run_loop_usage_limit_pause_workpad, AssigneeOwnershipDecision, MainSessionReconciliation,
    RunLoopClaimAction, RunLoopLiveHandoff,
};
pub(crate) use lanes::main_loop::{
    compact_evidence, linked_pull_requests_contain, main_app_server_smoke_gate,
    pull_request_number_from_url, reconcile_main_handoff_runtime_state, run_loop,
    run_loop_handoff_failure_workpad, run_loop_handoff_plan, run_once, RunLoopOptions,
};
#[cfg(test)]
use lanes::main_loop::{
    no_dispatch_action, run_loop_dispatch_write_candidates, select_main_run_loop_issues,
    NoDispatchAction, RuntimeRecoveryCandidate,
};
#[cfg(test)]
use lanes::main_loop::{
    run_loop_resume_preflight, run_loop_resume_preflight_many, runtime_state_issue_identifier,
    ResumePreflightAction,
};
pub(crate) use lanes::merge::MergeLoopOptions;
#[cfg(test)]
use lanes::merge::{
    finish_merge_agent_repaired_branch, merge_agent_reports_repaired,
    merge_agent_requests_human_input, merge_once_tick, record_done_merge_lane_completion,
    select_merge_worker_issues, MergeOnceOutcome,
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
pub(crate) use orchestration::canonical_checkout::{
    append_canonical_checkout_gap, enforce_canonical_checkout_before_write,
    preflight_canonical_checkout_for_write_mode, report_canonical_checkout_readonly,
};
#[cfg(test)]
use orchestration::canonical_checkout::{canonical_checkout_report, CanonicalCheckoutReport};
pub(crate) use orchestration::progress::{progress_spec_for_config, progress_spec_with_event_log};
pub(crate) use orchestration::session_status::{
    session_status_snapshots, DEFAULT_SESSION_STALE_AFTER_MS, DEFAULT_SESSION_STATUS_LINES,
};
pub(crate) use orchestration::text::{shell_quote_display, single_line};
pub(crate) use orchestration::time::{current_gmt_timestamp, current_time_ms};
pub(crate) use orchestration::tracker_context::{
    all_mapped_tracker_states, hydrate_issue_for_evidence, hydrate_issues_for_review_lane,
    live_github_tracker, tracker_backend_label,
};
pub(crate) use orchestration::tracker_recovery::{
    add_timeline_comment_with_recovery, append_tracker_mutation_audit, close_issue_with_recovery,
    merge_completion_recovery_key, merge_decision_recovery_key, merge_pull_request_with_recovery,
    recovery_key, set_project_field_with_recovery, set_state_with_recovery, stable_recovery_hash,
    upsert_workpad_with_recovery, TrackerMutationAudit, TrackerMutationOutcome,
};
#[cfg(test)]
use orchestration::tracker_recovery::{issue_is_closed, tracker_recovery_marker};
#[cfg(test)]
use orchestration::workflow_config::temporary_workflow_warning;
pub(crate) use orchestration::workflow_config::{
    load_config, require_write_intent, warn_if_temporary_workflow_path,
};

const DEFAULT_RUN_LOOP_BASE_BRANCH: &str = "main";
const CODEX_APP_SERVER_HANDOFF_BOUNDARY: &str = "\n\n## Codex App-Server Runtime Boundary\n\n\
This run is executing inside the Codex app-server backend. Treat the app-server \
turn as the implementation and local-verification worker only. Do not run \
GitHub Project reads or mutations, do not create or update pull requests, and \
do not attempt final Project state transitions from inside this child turn. \
Leave a concise terminal summary of changed files, verification commands, and \
any blocker. The outer Jade Symphony CLI will commit eligible worktree changes, \
publish or update the PR, write durable workpad evidence, verify linked PR \
readback, and perform the final `Agent Review` handoff.\n";
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

fn unbounded_loop_sleep_ms(limit: Option<usize>, poll_interval_ms: u64) -> Option<u64> {
    limit.is_none().then_some(poll_interval_ms)
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
