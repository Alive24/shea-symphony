use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod repair;

use jade_symphony::canonical_checkout::{
    canonical_checkout_status_line, inspect_canonical_checkout,
};
use jade_symphony::config::RuntimeConfig;
use jade_symphony::doctor::{
    append_local_skill_install_doctor_violations, audit_project_issues_with_context,
    default_jade_symphony_skill_targets, render_project_audit_report,
    render_project_audit_report_json, AuditSeverity, ProjectAuditReport, ProjectAuditViolation,
    ProjectDoctorContext, AGENT_REVIEW_DRAFT_PR,
};
use jade_symphony::issue_workspace::{discover_issue_workspaces_from_parts, git_worktree_list};
use jade_symphony::model::{native_subissue_statuses, TrackerIssue};
use jade_symphony::presentation::render_doctor_panel;
use jade_symphony::progress::run_with_progress_heartbeat;
use jade_symphony::runtime_state::load_runtime_states;
use jade_symphony::session_registry::{load_session_registry, session_registry_path};
use jade_symphony::skill_status::{doctor_skill_readiness_summary, SkillStatusInput};
use jade_symphony::tracker::{adapter_from_config, TrackerAdapter, TrackerError};
use jade_symphony::workflow::WorkflowDefinition;

use crate::cli::DisplayMode;
use crate::{
    all_mapped_tracker_states, append_canonical_checkout_gap, current_time_ms,
    progress_spec_for_config, session_status_snapshots, tracker_backend_label,
};

pub(crate) use repair::doctor_repair_human_review;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorOptions {
    pub(crate) workflow_path: Option<PathBuf>,
    pub(crate) json: bool,
    pub(crate) strict: bool,
    pub(crate) display: DisplayMode,
    pub(crate) interactive: bool,
    pub(crate) auto_fix: bool,
    pub(crate) write: bool,
    pub(crate) stale_after_ms: u64,
    pub(crate) action: Option<DoctorAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DoctorAction {
    Repair(DoctorRepairIssueOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorRepairIssueOptions {
    pub(crate) issue_ref: String,
    pub(crate) write: bool,
    pub(crate) move_need_human_input: bool,
    pub(crate) mark_pr_ready: bool,
    pub(crate) confirm_handoff_ready: bool,
}

pub(crate) fn hydrate_issues_for_doctor(
    adapter: &dyn TrackerAdapter,
    issues: Vec<TrackerIssue>,
) -> Result<Vec<TrackerIssue>, TrackerError> {
    let project_context = issues.clone();
    let rich_refs = doctor_issue_refs_requiring_rich_hydration(&issues);
    issues
        .into_iter()
        .map(|issue| {
            if rich_refs.contains(&issue.identifier) {
                adapter.hydrate_issue_evidence(issue, &project_context)
            } else {
                Ok(issue)
            }
        })
        .collect()
}

pub(crate) fn doctor_issue_refs_requiring_rich_hydration(
    issues: &[TrackerIssue],
) -> BTreeSet<String> {
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

pub(crate) fn doctor(options: DoctorOptions) -> Result<(), Box<dyn std::error::Error>> {
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
            repair::doctor_repair_issue(&config, adapter.as_ref(), &issues, &report, repair)?;
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
                repair::apply_doctor_auto_fix(&config, adapter.as_ref(), &report, options.write)?;
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

pub(crate) fn resolve_doctor_workflow_path(explicit: Option<PathBuf>) -> PathBuf {
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

pub(crate) fn discover_skill_suite_repo_root(
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

pub(crate) fn append_canonical_checkout_doctor_violations(
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

pub(crate) fn append_workspace_doctor_violations(
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

pub(crate) fn doctor_health_label(report: &ProjectAuditReport) -> &'static str {
    if report.blocker_count() > 0 {
        "blocked"
    } else if report.violations.is_empty() {
        "clean"
    } else {
        "needs_attention"
    }
}
