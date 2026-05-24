use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jade_symphony::artifacts::cleanup_plan;
use jade_symphony::config::RuntimeConfig;
use jade_symphony::doctor::{
    audit_project_issues_with_context, ProjectAuditReport, ProjectDoctorContext,
};
use jade_symphony::lane_claim::{LaneClaim, LaneClaimLane, LaneClaimState};
use jade_symphony::model::{normalize_state, SessionStatusSnapshot, TrackerIssue};
use jade_symphony::runtime_state::{load_runtime_states, runtime_state_path};
use jade_symphony::session_registry::session_registry_path;
use jade_symphony::tracker::adapter_from_config;
use jade_symphony::workflow::WorkflowDefinition;

use crate::commands::doctor::{
    append_workspace_doctor_violations, doctor_health_label, hydrate_issues_for_doctor,
};
use crate::commands::gate::evaluate_issue_for_current_source;
use crate::commands::project::render_state_summary;
use crate::lanes::claim::project_text_field;
use crate::lanes::main_loop::main_app_server_smoke_gate;
use crate::orchestration::{
    all_mapped_tracker_states, current_time_ms, session_status_snapshots, shell_quote_display,
    warn_if_temporary_workflow_path,
};

pub(crate) fn debug_report(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
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

pub(crate) fn session_status_summary(sessions: &[SessionStatusSnapshot]) -> String {
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

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DogfoodIntegrationGapReport {
    pub(crate) blocking: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) fn classify_dogfood_integration_gaps(gaps: &[String]) -> DogfoodIntegrationGapReport {
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

pub(crate) fn is_controlled_dogfood_smoke_issue(issue: &TrackerIssue) -> bool {
    issue
        .labels_lowercase()
        .iter()
        .any(|label| label == "dogfood-smoke" || label == "smoke")
        || issue.title.to_ascii_lowercase().contains("[dogfood-smoke]")
}
