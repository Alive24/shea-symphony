use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use shea_symphony::issue_workspace::{
    discover_issue_workspaces_from_parts, git_worktree_list, render_workspace_ensure_workpad,
    validate_workspace_adoption, IssueWorkspaceCandidate, WorkspaceMatchStrength,
};
use shea_symphony::model::TrackerIssue;
use shea_symphony::session_registry::{load_session_registry, session_registry_path};
use shea_symphony::tracker::adapter_from_config;

use crate::lanes::main_loop::{pull_request_number_from_url, run_loop_handoff_plan};
use crate::orchestration::{
    append_tracker_mutation_audit, current_git_branch, load_config,
    preflight_canonical_checkout_for_write_mode, TrackerMutationAudit,
};

pub(crate) fn workspace_ensure(
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

pub(crate) fn validate_workspace_path_under_root(
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

pub(crate) fn ensure_inspection_worktree(
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
