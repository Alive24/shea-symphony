use std::path::PathBuf;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::handoff::plan_issue_handoff_for_profile;
use jade_symphony::issue_workspace::{
    discover_issue_workspaces_from_parts, git_worktree_list, infer_issue_ref_from_branch_or_path,
    render_workspace_adoption_workpad, validate_workspace_adoption, IssueWorkspaceCandidate,
    IssueWorkspaceReport,
};
use jade_symphony::model::TrackerIssue;
use jade_symphony::profiles::selected_execution_profile;
use jade_symphony::session_registry::{load_session_registry, session_registry_path};
use jade_symphony::tracker::adapter_from_config;
use jade_symphony::workspace::remove_issue_workspace;

use crate::{
    all_mapped_tracker_states, append_tracker_mutation_audit, load_config, TrackerMutationAudit,
    DEFAULT_RUN_LOOP_BASE_BRANCH,
};

mod ensure;

pub(crate) use ensure::workspace_ensure;
#[cfg(test)]
pub(crate) use ensure::{ensure_inspection_worktree, validate_workspace_path_under_root};

pub(crate) fn cleanup_workspaces(
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

pub(crate) fn workspace_list(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
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

pub(crate) fn workspace_show(
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

pub(crate) fn workspace_adopt(
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
pub(crate) struct WorkspaceCleanupEntry {
    pub(crate) issue_ref: String,
    pub(crate) state: String,
    pub(crate) workspace_key: String,
    pub(crate) workspace_path: PathBuf,
    pub(crate) action: WorkspaceCleanupAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceCleanupAction {
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

pub(crate) fn workspace_cleanup_plan(
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
