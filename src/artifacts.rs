use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::config::RuntimeConfig;
use crate::handoff::plan_issue_handoff_for_profile;
use crate::model::{normalize_state, TrackerIssue};
use crate::profiles::selected_execution_profile;
use crate::workspace::safe_identifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactClass {
    PerIssueWorktree,
    RuntimeState,
    EventLog,
    ReviewJobArtifact,
    PullRequestBodyDraft,
    WorkpadDraft,
    ReusableWorkflowPrompt,
    DisposableScratch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactLayout {
    pub root: PathBuf,
    pub namespace: String,
    pub profile_namespace: String,
    pub worktrees: PathBuf,
    pub runtime: PathBuf,
    pub logs: PathBuf,
    pub reviews: PathBuf,
    pub pr_bodies: PathBuf,
    pub workpads: PathBuf,
    pub workflows: PathBuf,
    pub scratch: PathBuf,
}

impl ArtifactLayout {
    pub fn class_path(&self, class: ArtifactClass) -> &Path {
        match class {
            ArtifactClass::PerIssueWorktree => &self.worktrees,
            ArtifactClass::RuntimeState => &self.runtime,
            ArtifactClass::EventLog => &self.logs,
            ArtifactClass::ReviewJobArtifact => &self.reviews,
            ArtifactClass::PullRequestBodyDraft => &self.pr_bodies,
            ArtifactClass::WorkpadDraft => &self.workpads,
            ArtifactClass::ReusableWorkflowPrompt => &self.workflows,
            ArtifactClass::DisposableScratch => &self.scratch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupPlan {
    pub workspace_root: PathBuf,
    pub candidates: Vec<CleanupCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupCandidate {
    pub issue_identifier: String,
    pub issue_state: String,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub linked_pr_state: Option<String>,
    pub removable: bool,
    pub reasons: Vec<String>,
    pub blockers: Vec<String>,
}

pub fn artifact_layout(config: &RuntimeConfig) -> ArtifactLayout {
    let namespace = artifact_namespace(config);
    let profile_namespace = selected_execution_profile(&config.profiles)
        .ok()
        .flatten()
        .map(|profile| profile.workspace_namespace)
        .or_else(|| config.profiles.default.clone())
        .unwrap_or_else(|| "default".to_string());
    let root = config
        .artifacts
        .root
        .join(&namespace)
        .join(safe_identifier(&profile_namespace));
    ArtifactLayout {
        worktrees: root.join("worktrees"),
        runtime: root.join("runtime"),
        logs: root.join("logs"),
        reviews: root.join("reviews"),
        pr_bodies: root.join("drafts").join("pr-bodies"),
        workpads: root.join("drafts").join("workpads"),
        workflows: root.join("workflows"),
        scratch: root.join("scratch"),
        root,
        namespace,
        profile_namespace,
    }
}

pub fn artifact_namespace(config: &RuntimeConfig) -> String {
    if let Some(namespace) = config
        .artifacts
        .namespace
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return safe_identifier(namespace);
    }

    if let (Some(owner), Some(repo)) = (&config.tracker.owner, &config.tracker.repo) {
        return format!("{}/{}", safe_identifier(owner), safe_identifier(repo));
    }

    config
        .tracker
        .project_slug
        .as_deref()
        .map(safe_identifier)
        .unwrap_or_else(|| "local".to_string())
}

pub fn cleanup_plan(config: &RuntimeConfig, issues: &[TrackerIssue]) -> CleanupPlan {
    let profile_namespace = selected_execution_profile(&config.profiles)
        .ok()
        .flatten()
        .map(|profile| profile.workspace_namespace);
    let terminal_states = config
        .terminal_state_set()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let candidates = issues
        .iter()
        .map(|issue| {
            cleanup_candidate(
                &config.workspace.root,
                issue,
                &terminal_states,
                profile_namespace.as_deref(),
            )
        })
        .collect();

    CleanupPlan {
        workspace_root: config.workspace.root.clone(),
        candidates,
    }
}

fn cleanup_candidate(
    workspace_root: &Path,
    issue: &TrackerIssue,
    terminal_states: &BTreeSet<String>,
    profile_namespace: Option<&str>,
) -> CleanupCandidate {
    let handoff = plan_issue_handoff_for_profile(workspace_root, issue, "main", profile_namespace);
    let path = handoff
        .as_ref()
        .map(|handoff| handoff.workspace_path.clone())
        .unwrap_or_else(|_| workspace_root.join(safe_identifier(&issue.identifier)));
    let mut reasons = Vec::new();
    let mut blockers = Vec::new();

    if terminal_states.contains(&normalize_state(&issue.state)) {
        reasons.push(format!("tracker state is terminal: {}", issue.state));
    } else {
        blockers.push(format!("tracker state is not terminal: {}", issue.state));
    }

    if path.exists() {
        reasons.push("workspace path exists".into());
    } else {
        blockers.push("workspace path does not exist".into());
    }

    let branch = git_current_branch(&path);
    if let Ok(handoff) = &handoff {
        match branch.as_deref() {
            Some(branch) if branch == handoff.branch_name => {
                reasons.push(format!("branch matches issue handoff: {branch}"));
            }
            Some(branch) => blockers.push(format!(
                "workspace branch `{branch}` does not match `{}`",
                handoff.branch_name
            )),
            None => blockers.push("workspace git branch is unavailable".into()),
        }
    } else if let Err(error) = &handoff {
        blockers.push(format!("handoff plan failed: {error}"));
    }

    if workspace_clean(&path) {
        reasons.push("workspace has no uncommitted changes".into());
    } else {
        blockers.push("workspace has uncommitted changes or git status is unavailable".into());
    }

    let linked_pr_state = merged_or_closed_pr_state(issue);
    if let Some(state) = &linked_pr_state {
        reasons.push(format!("linked PR is {state}"));
    } else {
        blockers.push("no linked PR in merged or closed state".into());
    }

    CleanupCandidate {
        issue_identifier: issue.identifier.clone(),
        issue_state: issue.state.clone(),
        path,
        branch,
        linked_pr_state,
        removable: blockers.is_empty(),
        reasons,
        blockers,
    }
}

fn merged_or_closed_pr_state(issue: &TrackerIssue) -> Option<String> {
    issue.linked_pull_requests.iter().find_map(|pr| {
        let state = pr.state.as_deref()?.trim().to_ascii_lowercase();
        matches!(state.as_str(), "merged" | "closed").then_some(state)
    })
}

fn git_current_branch(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", path.to_str()?, "branch", "--show-current"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|branch| !branch.is_empty())
}

fn workspace_clean(path: &Path) -> bool {
    let Some(path) = path.to_str() else {
        return false;
    };
    let Ok(output) = Command::new("git")
        .args(["-C", path, "status", "--porcelain"])
        .output()
    else {
        return false;
    };
    output.status.success() && output.stdout.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LinkedPullRequest;
    use crate::workflow::WorkflowDefinition;

    fn config(markdown: &str) -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse("/tmp/WORKFLOW.md", markdown).unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn issue(state: &str, pr_state: Option<&str>) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: "GHI_158".into(),
            item_id: None,
            identifier: "#158".into(),
            title: "Harden runtime artifact storage and cleanup policy".into(),
            description: None,
            url: None,
            state: state.into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: None,
            linked_pull_requests: pr_state
                .map(|state| LinkedPullRequest {
                    id: None,
                    number: Some(158),
                    url: Some("https://github.com/Alive24/jade-symphony/pull/158".into()),
                    state: Some(state.into()),
                })
                .into_iter()
                .collect(),
            blocked_by: Vec::new(),
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn builds_namespaced_layout_from_repo_and_profile() {
        let config = config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\nartifacts:\n  root: /tmp/artifacts\nprofiles:\n  default: codex-alpha\n---\nPrompt",
        );

        let layout = artifact_layout(&config);

        assert_eq!(layout.namespace, "Alive24/jade-symphony");
        assert!(layout
            .worktrees
            .ends_with("Alive24/jade-symphony/codex-alpha/worktrees"));
        assert!(layout.runtime.ends_with("runtime"));
        assert_eq!(
            layout.class_path(ArtifactClass::PullRequestBodyDraft),
            layout.pr_bodies.as_path()
        );
    }

    #[test]
    fn cleanup_plan_requires_terminal_pr_branch_and_clean_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let config = config(&format!(
            "---\ntracker:\n  kind: memory\nworkspace:\n  root: {:?}\n---\nPrompt",
            temp.path().display().to_string()
        ));
        let issue = issue("Done", Some("MERGED"));
        let path = temp
            .path()
            .join("issue-158-harden-runtime-artifact-storage-and-cleanup-policy");
        std::fs::create_dir_all(&path).unwrap();
        Command::new("git").arg("init").arg(&path).output().unwrap();
        Command::new("git")
            .args([
                "-C",
                path.to_str().unwrap(),
                "checkout",
                "-b",
                "feature/issue-158-harden-runtime-artifact-storage-and-cleanup-policy",
            ])
            .output()
            .unwrap();

        let plan = cleanup_plan(&config, &[issue]);

        assert_eq!(plan.candidates.len(), 1);
        let candidate = &plan.candidates[0];
        assert!(candidate.removable);
        assert!(candidate
            .reasons
            .iter()
            .any(|reason| reason.contains("tracker state is terminal")));
    }

    #[test]
    fn cleanup_plan_blocks_dirty_or_unmerged_workspaces() {
        let temp = tempfile::tempdir().unwrap();
        let config = config(&format!(
            "---\ntracker:\n  kind: memory\nworkspace:\n  root: {:?}\n---\nPrompt",
            temp.path().display().to_string()
        ));
        let issue = issue("Done", Some("OPEN"));

        let plan = cleanup_plan(&config, &[issue]);

        let candidate = &plan.candidates[0];
        assert!(!candidate.removable);
        assert!(candidate
            .blockers
            .iter()
            .any(|blocker| blocker.contains("no linked PR")));
        assert!(candidate
            .blockers
            .iter()
            .any(|blocker| blocker.contains("workspace path does not exist")));
    }
}
