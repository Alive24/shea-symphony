use super::*;
use shea_symphony::tracker::MemoryTracker;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use super::cli::{CliLaneClaimSource, DisplayMode, ForgeStatusArg};
use super::commands::autopilot::{
    autopilot_loop_status_from_plan, build_autopilot_plan_from_parts, AutopilotActiveIssue,
    AutopilotCanonicalCheckout, AutopilotIssueSummary, AutopilotLanePlan, AutopilotLoopSettings,
    AutopilotPlanInputs, AutopilotPlanSnapshot, AutopilotRuntimeSummary,
};
use super::commands::debug::{
    classify_dogfood_integration_gaps, is_controlled_dogfood_smoke_issue, session_status_summary,
};
use super::commands::doctor::{
    doctor_health_label, hydrate_issues_for_doctor, DoctorAction, DoctorOptions,
    DoctorRepairIssueOptions,
};
use super::commands::forge::{
    apply_forge_relationship_plan, find_duplicate_issue_title, forge_create_requires_assignee,
    forge_missing_categories, forge_promote, forge_rework_with_adapter, forge_validation_report,
    issue_contract_assignees, render_forge_create_success, render_promotion_note,
    validate_forge_create_contract, validate_forge_create_report_with_assignees,
    verify_forge_created_issue_status, write_forge_created_issue, ForgeCreateResult,
    ForgeCreateWriteInput, ForgePromoteInput, ForgeRelationshipPlan, ForgeReworkInput,
    PromotionNoteInput,
};
use super::commands::gate::live_missing_assignee_gate_blocker;
use super::commands::project::{
    filter_issues_by_state, link_pr_with_adapter, project_state_issues_for_scope,
    render_project_state_json, render_state_summary, ProjectStateOptions,
};
use super::commands::session::{
    agent_session_backend_spec, lane_claim_for_manual_worker, matching_lane_claim_for_session,
    record_manual_lane_claim_evidence, tmux_agent_command_for_lane, validate_lane_claim_state,
};
use super::commands::status::render_plan_snapshot;
use super::commands::workspace::{
    ensure_inspection_worktree, validate_workspace_path_under_root, workspace_cleanup_plan,
    WorkspaceCleanupAction,
};
use super::lanes::claim::{
    pool_claim_eligibility, project_text_field, render_parseable_lane_claim,
    select_pool_worker_issues, terminal_lane_claim_value, write_lane_claim_field,
    PoolClaimEligibility, WorkerLane,
};
use super::lanes::main_loop::{
    apply_live_handoff_pr_link, execute_issue_once_with_workspace_key, main_app_server_smoke_gate,
    no_dispatch_action, pull_request_number_from_url, reconcile_main_handoff_runtime_state,
    reconcile_pending_main_session, run_handoff_verification,
    run_loop_agent_review_handoff_evidence, run_loop_apply_launch_workspace_report,
    run_loop_apply_recovery_handoff, run_loop_assignee_ownership_decision, run_loop_claim_action,
    run_loop_dispatch_write_candidates, run_loop_handoff_plan, run_loop_handoff_workpad,
    run_loop_ownership_workpad, run_loop_resume_preflight, run_loop_resume_preflight_many,
    run_loop_runtime_ownership, run_loop_runtime_state_for_issue,
    run_loop_runtime_state_with_result, run_loop_runtime_state_with_transition,
    run_loop_usage_limit_pause_workpad, runtime_state_issue_identifier,
    select_main_run_loop_issues, AssigneeOwnershipDecision, IssueExecutionResult,
    MainSessionReconciliation, NoDispatchAction, ResumePreflightAction, RunLoopClaimAction,
    RunLoopLiveHandoff, RunLoopOptions, RuntimeRecoveryCandidate,
};
use super::lanes::merge::{
    finish_merge_agent_repaired_branch, merge_agent_reports_repaired,
    merge_agent_requests_human_input, merge_once_tick, record_done_merge_lane_completion,
    select_merge_worker_issues, stage_resolved_merge_agent_changes, MergeAgentStageFailure,
    MergeOnceOutcome, MergeTickOutputScope,
};
use super::lanes::review::{
    apply_review_result, canonical_issue_body_without_workpad,
    check_review_verified_issue_body_checkboxes, render_automatic_review_prompt,
    render_manual_review_workpad, review_claim_for_issue, review_workspace_for_issue,
    select_review_worker_issues, terminal_review_claim_value, terminal_review_loop_claim_value,
    transition_issue_to_rework_with_diagnostic, validate_active_manual_review_claim,
    validate_manual_review_pass_claim,
};
use super::orchestration::canonical_checkout::{
    canonical_checkout_report, CanonicalCheckoutReport,
};
use super::orchestration::tracker_recovery::{issue_is_closed, tracker_recovery_marker};
use super::orchestration::workflow_config::temporary_workflow_warning;
use super::orchestration::{
    add_timeline_comment_with_recovery, all_mapped_tracker_states, append_tracker_mutation_audit,
    close_issue_with_recovery, current_git_branch, merge_pull_request_with_recovery, recovery_key,
    set_state_with_recovery, TrackerMutationAudit, TrackerMutationOutcome,
};
use shea_symphony::agent::UsageLimitPause;
use shea_symphony::config::RuntimeConfig;
use shea_symphony::doctor::ProjectAuditViolation;
use shea_symphony::doctor::{AuditSeverity, ProjectAuditReport};
use shea_symphony::event_log::EventLog;
use shea_symphony::git_handoff::{CommandOutput, HandoffCommandRunner};
use shea_symphony::git_handoff::{
    LiveWorktreeResult, PullRequestPublication, PullRequestReadyStatus,
};
use shea_symphony::handoff::evaluate_agent_review_handoff;
use shea_symphony::handoff::{plan_issue_handoff_for_profile, HandoffError, IssueHandoffPlan};
use shea_symphony::issue_workspace::{
    IssueWorkspaceCandidate, IssueWorkspaceReport, WorkspaceEvidence, WorkspaceMatchStrength,
};
use shea_symphony::lane_claim::{
    LaneClaim, LaneClaimActor, LaneClaimLane, LaneClaimSource, LaneClaimState,
};
use shea_symphony::merge_lane::repair_dirty_pull_request;
use shea_symphony::model::{normalize_state, LinkedPullRequest};
use shea_symphony::model::{BlockerRef, SessionStatusSnapshot, TrackerIssue};
use shea_symphony::orchestrator::Orchestrator;
use shea_symphony::ownership::render_runtime_ownership_marker;
use shea_symphony::ownership::{runtime_ownership_decision, RuntimeOwnershipDecision};
use shea_symphony::review::FakeReviewOutcome;
use shea_symphony::review::{
    ReviewGateDecision, ReviewJob, ReviewJobState, ReviewOutcome, ReviewReworkClass,
    ReviewStaleReason,
};
use shea_symphony::rework::ReworkDiagnostic;
use shea_symphony::runtime_state::{
    load_runtime_states, record_runtime_retry, runtime_state_for_issue, upsert_runtime_state,
};
use shea_symphony::runtime_state::{RuntimeIssueState, RuntimeState, RuntimeTransition};
use shea_symphony::session_registry::AgentSessionRecord;
use shea_symphony::session_registry::{load_session_registry, save_session_record};
use shea_symphony::session_registry::{
    save_session_registry, session_registry_path, SessionStatus,
};
use shea_symphony::skill_status::SkillStatusInput;
use shea_symphony::tracker::FollowUpIssueInput;
use shea_symphony::tracker::ProjectFieldAssignment;
use shea_symphony::tracker::TrackerAdapter;
use shea_symphony::workflow::WorkflowDefinition;
use shea_symphony::workspace::GitIdentityApplyResult;

#[path = "tests/autopilot.rs"]
mod autopilot;
#[path = "tests/forge.rs"]
mod forge;
#[path = "tests/main_loop.rs"]
mod main_loop;
#[path = "tests/merge.rs"]
mod merge;
#[path = "tests/parser.rs"]
mod parser;
#[path = "tests/review.rs"]
mod review;
#[path = "tests/support.rs"]
mod support;

pub(crate) use support::*;

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
    git_ok(&other, &["config", "user.email", "shea@example.invalid"]);
    git_ok(&other, &["config", "user.name", "Shea Symphony"]);
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
    config.identity.actor_label = "Shea Symphony Merge Worker".into();

    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "merge once",
            mutation_type: "state_change",
            issue_ref: Some("#7"),
            target: Some("https://github.com/Alive24/shea-symphony/pull/7".into()),
            from_state: Some("Merging".into()),
            to_state: Some("Done".into()),
            reason: "merge completed",
        },
    );

    let records = EventLog::new(config.observability.logs_root.join("shea-symphony.jsonl"))
        .read_records()
        .unwrap();
    let record = records.first().expect("expected audit record");
    assert_eq!(record.event, "tracker_mutation");
    assert_eq!(record.actor_role.as_deref(), Some("merge_agent"));
    assert_eq!(
        record.actor_label.as_deref(),
        Some("Shea Symphony Merge Worker")
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
        .join("SHEA_SYMPHONY_PROMPT.md")
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

    let records = EventLog::new(logs_root.join("shea-symphony.jsonl"))
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
        temporary_workflow_warning(Path::new("/private/tmp/shea-github-project-workflow.md"))
            .expect("expected temporary workflow warning");

    assert!(warning.contains("workflow_warning=temporary_path"));
    assert!(warning.contains("action=promote"));
    assert!(temporary_workflow_warning(Path::new("examples/github-project-workflow.md")).is_none());
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
    .with_worker("Shea Symphony Agent");
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
    let workpad = "## Shea Symphony Merge Run\n\n- Result: `merged_or_done`";

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
        "https://github.com/Alive24/shea-symphony/pull/351",
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
fn renders_project_state_json_queue_projection() {
    let issues = vec![
        tracker_issue_with_ref("#1", "Ready", "Todo"),
        tracker_issue_with_ref("#2", "Review", "Agent Review"),
        tracker_issue_with_ref("#3", "Approval", "Human Review"),
    ];

    let rendered = render_project_state_json(
        &issues,
        &["gap".into()],
        "queue",
        &BTreeSet::from(["done".into()]),
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(value["trusted"], true);
    assert_eq!(value["scope"], "queue");
    assert_eq!(value["totalOpen"], 3);
    assert_eq!(value["laneCounts"]["main"], 1);
    assert_eq!(value["laneCounts"]["review"], 1);
    assert_eq!(value["operatorIssues"][0]["identifier"], "#3");
    assert_eq!(value["integrationGaps"][0], "gap");
}

#[test]
fn renders_project_state_json_dependency_readback_without_main_lane_projection() {
    let ready = tracker_issue_with_ref("#1", "Ready", "Todo");
    let mut blocked = tracker_issue_with_ref("#2", "Blocked", "Todo");
    blocked.blocked_by = vec![BlockerRef {
        id: Some("I_kwBLOCKER".into()),
        identifier: Some("#9".into()),
        state: Some("Todo".into()),
    }];
    let mut resolved = tracker_issue_with_ref("#3", "Resolved dependency", "Rework");
    resolved.blocked_by = vec![BlockerRef {
        id: None,
        identifier: Some("#8".into()),
        state: Some("Done".into()),
    }];
    let terminal_states = BTreeSet::from(["done".into()]);

    let rendered =
        render_project_state_json(&[ready, blocked, resolved], &[], "queue", &terminal_states)
            .unwrap();
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(value["laneCounts"]["main"], 2);
    assert_eq!(value["issues"][1]["identifier"], "#2");
    assert_eq!(value["issues"][1]["blockedBy"][0]["identifier"], "#9");
    assert_eq!(value["issues"][1]["blockedBy"][0]["state"], "Todo");
    assert_eq!(
        value["issues"][1]["blockedReason"],
        "issue has tracker dependencies"
    );
    assert_eq!(value["issues"][2]["blockedBy"][0]["state"], "Done");
}

#[test]
fn renders_project_state_json_native_parent_gate_without_main_lane_projection() {
    let mut incomplete = tracker_issue_with_ref("#447", "Incomplete native parent", "Todo");
    incomplete.project_fields.insert(
        "GitHub Native Subissues".into(),
        serde_json::json!([
            {"identifier": "#450", "project_state": "Done"},
            {"identifier": "#451", "project_state": "Agent Review"}
        ]),
    );
    let mut complete = tracker_issue_with_ref("#448", "Complete native parent", "Todo");
    complete.project_fields.insert(
        "GitHub Native Subissues".into(),
        serde_json::json!([
            {"identifier": "#452", "project_state": "Done"},
            {"identifier": "#453", "project_state": "Done"}
        ]),
    );
    let terminal_states = BTreeSet::from(["done".into()]);

    let rendered =
        render_project_state_json(&[incomplete, complete], &[], "queue", &terminal_states).unwrap();
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(value["laneCounts"]["main"], 1);
    assert_eq!(
        value["issues"][0]["nativeSubissues"][1]["identifier"],
        "#451"
    );
    assert_eq!(
        value["issues"][0]["nativeSubissues"][1]["projectState"],
        "Agent Review"
    );
}

#[test]
fn project_state_queue_scope_excludes_terminal_issues_by_default() {
    let workflow = WorkflowDefinition::parse(
        "/tmp/WORKFLOW.md",
        "---\ntracker:\n  kind: memory\n  terminal_states:\n    - Done\n    - Closed\n---\nPrompt",
    )
    .unwrap();
    let config =
        RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md")).unwrap();
    let issues = vec![
        tracker_issue_with_ref("#1", "Ready", "Todo"),
        tracker_issue_with_ref("#2", "Review", "Agent Review"),
        tracker_issue_with_ref("#3", "Done", "Done"),
    ];

    let queue = project_state_issues_for_scope(issues.clone(), &config, false);
    assert_eq!(
        queue
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        vec!["#1", "#2"]
    );

    let all = project_state_issues_for_scope(issues, &config, true);
    assert_eq!(all.len(), 3);
}

#[test]
fn renders_plan_snapshot_as_json_when_requested() {
    let snapshot = shea_symphony::model::RuntimeSnapshot {
        event_log_path: Some("/tmp/shea-symphony.jsonl".into()),
        integration_gaps: vec!["gap".into()],
        ..Default::default()
    };

    let rendered = render_plan_snapshot(&snapshot, true).unwrap();
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(
        value
            .get("event_log_path")
            .and_then(serde_json::Value::as_str),
        Some("/tmp/shea-symphony.jsonl")
    );
    assert_eq!(
        value
            .pointer("/integration_gaps/0")
            .and_then(serde_json::Value::as_str),
        Some("gap")
    );
}

#[test]
fn main_session_defaults_to_codex_app_server_command() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: codex\ncodex:\n  command: /opt/homebrew/bin/codex app-server\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

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
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    let spec = agent_session_backend_spec(&config, AgentSessionLaneArg::Main).unwrap();

    assert_eq!(spec.backend, "tmux");
    assert_eq!(spec.command, "codex --profile main");
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
