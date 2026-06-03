use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;
use crate::model::TrackerIssue;
use crate::session_registry::{load_session_registry, session_registry_path, AgentSessionRecord};

#[derive(Debug, Error)]
pub enum IssueWorkspaceError {
    #[error("workspace discovery io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("workspace discovery session registry error: {0}")]
    SessionRegistry(#[from] crate::session_registry::SessionRegistryError),
    #[error("git worktree list failed with status {status}: {stderr}")]
    GitWorktreeList { status: i32, stderr: String },
    #[error("path is not a worktree for this repository: {0}")]
    NotRepositoryWorktree(PathBuf),
    #[error("worktree has no branch and cannot be adopted safely: {0}")]
    DetachedWorktree(PathBuf),
    #[error("worktree branch `{branch}` does not match issue {issue}")]
    BranchMismatch { issue: String, branch: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueWorkspaceReport {
    pub issue_ref: String,
    pub title: String,
    #[serde(default)]
    pub branch_hints: Vec<String>,
    #[serde(default)]
    pub candidates: Vec<IssueWorkspaceCandidate>,
    #[serde(default)]
    pub canonical_index: Option<usize>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueWorkspaceCandidate {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub strength: WorkspaceMatchStrength,
    #[serde(default)]
    pub evidence: Vec<WorkspaceEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMatchStrength {
    Weak,
    Strong,
}

impl WorkspaceMatchStrength {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Weak => "weak",
            Self::Strong => "strong",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceEvidence {
    pub source: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub prunable: Option<String>,
}

pub fn discover_issue_workspaces(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    repo_root: &Path,
) -> Result<IssueWorkspaceReport, IssueWorkspaceError> {
    let registry = load_session_registry(&session_registry_path(config))?;
    let worktrees = git_worktree_list(repo_root)?;
    Ok(discover_issue_workspaces_from_parts(
        issue,
        &registry.sessions,
        &worktrees,
        config.tracker.workpad.marker.as_str(),
    ))
}

pub fn git_worktree_list(repo_root: &Path) -> Result<Vec<GitWorktree>, IssueWorkspaceError> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .output()?;
    if !output.status.success() {
        return Err(IssueWorkspaceError::GitWorktreeList {
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(parse_git_worktree_porcelain(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub fn parse_git_worktree_porcelain(input: &str) -> Vec<GitWorktree> {
    let mut worktrees = Vec::new();
    let mut current: Option<GitWorktree> = None;

    for line in input.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(worktree) = current.take() {
                worktrees.push(worktree);
            }
            current = Some(GitWorktree {
                path: PathBuf::from(path),
                head: None,
                branch: None,
                prunable: None,
            });
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            if let Some(worktree) = &mut current {
                worktree.head = Some(head.to_string());
            }
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if let Some(worktree) = &mut current {
                worktree.branch = Some(branch.trim_start_matches("refs/heads/").to_string());
            }
        } else if let Some(reason) = line.strip_prefix("prunable ") {
            if let Some(worktree) = &mut current {
                worktree.prunable = Some(reason.to_string());
            }
        }
    }

    if let Some(worktree) = current {
        worktrees.push(worktree);
    }

    worktrees
}

pub fn discover_issue_workspaces_from_parts(
    issue: &TrackerIssue,
    sessions: &[AgentSessionRecord],
    worktrees: &[GitWorktree],
    workpad_marker: &str,
) -> IssueWorkspaceReport {
    let mut candidates = BTreeMap::<PathBuf, IssueWorkspaceCandidate>::new();
    let hints = issue_workspace_hints(issue);
    let number_token = issue_number_token(&issue.identifier);
    let slug = issue_slug(issue);
    let worktree_by_path = worktrees
        .iter()
        .map(|worktree| (worktree.path.clone(), worktree))
        .collect::<BTreeMap<_, _>>();

    for session in sessions
        .iter()
        .filter(|session| session_matches_issue(session, &issue.identifier))
        .filter(|session| session.session_source.as_deref() != Some("manual-claim"))
    {
        let git = worktree_by_path.get(&session.worktree);
        upsert_candidate(
            &mut candidates,
            session.worktree.clone(),
            session
                .branch
                .clone()
                .or_else(|| git.and_then(|w| w.branch.clone())),
            git.and_then(|worktree| worktree.head.clone()),
            WorkspaceMatchStrength::Strong,
            WorkspaceEvidence {
                source: "session_registry".into(),
                detail: format!(
                    "{} lane={} status={}",
                    session.session_name,
                    session.lane,
                    session.status.as_str()
                ),
            },
        );
    }

    if let Some(description) = issue.description.as_deref() {
        for hint in workpad_workspace_hints(description, workpad_marker) {
            let git = worktree_by_path.get(&hint);
            let strong = description.contains("shea-symphony-workspace-adoption")
                || description.contains("shea-symphony-workspace-ensure")
                || description.contains("Workspace adoption");
            upsert_candidate(
                &mut candidates,
                hint,
                git.and_then(|worktree| worktree.branch.clone()),
                git.and_then(|worktree| worktree.head.clone()),
                if strong {
                    WorkspaceMatchStrength::Strong
                } else {
                    WorkspaceMatchStrength::Weak
                },
                WorkspaceEvidence {
                    source: "workpad".into(),
                    detail: "workspace path mentioned in issue workpad".into(),
                },
            );
        }
    }

    for worktree in worktrees {
        let mut evidence = Vec::new();
        let branch = worktree.branch.as_deref().unwrap_or_default();
        let path = worktree.path.to_string_lossy();
        let mut strength = None;

        for hint in &hints {
            if !hint.is_empty() && branch == hint {
                evidence.push(WorkspaceEvidence {
                    source: "git_worktree".into(),
                    detail: format!("branch matches hint `{hint}`"),
                });
                strength = Some(WorkspaceMatchStrength::Strong);
            }
        }

        if let Some(token) = &number_token {
            if branch.contains(token) || path.contains(token) {
                evidence.push(WorkspaceEvidence {
                    source: "git_worktree".into(),
                    detail: format!("branch or path contains `{token}`"),
                });
                strength = Some(WorkspaceMatchStrength::Strong);
            }
        }

        if !slug.is_empty() && (branch.contains(&slug) || path.contains(&slug)) {
            evidence.push(WorkspaceEvidence {
                source: "git_worktree".into(),
                detail: format!("branch or path contains issue slug `{slug}`"),
            });
            strength = strength.or(Some(WorkspaceMatchStrength::Weak));
        }

        if let Some(strength) = strength {
            for evidence in evidence {
                upsert_candidate(
                    &mut candidates,
                    worktree.path.clone(),
                    worktree.branch.clone(),
                    worktree.head.clone(),
                    strength,
                    evidence,
                );
            }
        }
    }

    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .strength
            .cmp(&left.strength)
            .then_with(|| left.path.cmp(&right.path))
    });
    let strong_count = candidates
        .iter()
        .filter(|candidate| candidate.strength == WorkspaceMatchStrength::Strong)
        .count();
    let canonical_index = if strong_count == 1 {
        candidates
            .iter()
            .position(|candidate| candidate.strength == WorkspaceMatchStrength::Strong)
    } else if strong_count == 0 && candidates.len() == 1 {
        Some(0)
    } else {
        None
    };
    let mut warnings = Vec::new();
    if strong_count > 1 {
        warnings.push(format!(
            "multiple strong workspace candidates for {}; operator choice is required",
            issue.identifier
        ));
    }

    IssueWorkspaceReport {
        issue_ref: issue.identifier.clone(),
        title: issue.title.clone(),
        branch_hints: hints,
        candidates,
        canonical_index,
        warnings,
    }
}

pub fn validate_workspace_adoption(
    issue: &TrackerIssue,
    path: &Path,
    worktrees: &[GitWorktree],
) -> Result<IssueWorkspaceCandidate, IssueWorkspaceError> {
    let expected = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let worktree = worktrees
        .iter()
        .find(|worktree| {
            worktree
                .path
                .canonicalize()
                .unwrap_or_else(|_| worktree.path.clone())
                == expected
        })
        .ok_or_else(|| IssueWorkspaceError::NotRepositoryWorktree(path.to_path_buf()))?;

    let Some(branch) = worktree.branch.clone() else {
        return Err(IssueWorkspaceError::DetachedWorktree(path.to_path_buf()));
    };

    if !branch_matches_issue(issue, &branch) {
        return Err(IssueWorkspaceError::BranchMismatch {
            issue: issue.identifier.clone(),
            branch,
        });
    }

    Ok(IssueWorkspaceCandidate {
        path: worktree.path.clone(),
        branch: worktree.branch.clone(),
        head: worktree.head.clone(),
        strength: WorkspaceMatchStrength::Strong,
        evidence: vec![WorkspaceEvidence {
            source: "operator_adoption".into(),
            detail: "validated local git worktree for this repository and issue branch".into(),
        }],
    })
}

pub fn render_workspace_adoption_workpad(
    issue: &TrackerIssue,
    marker: &str,
    candidate: &IssueWorkspaceCandidate,
) -> String {
    let block = format!(
        "<!-- shea-symphony-workspace-adoption -->\n### Workspace Adoption\n- Issue: `{}`\n- Path: `{}`\n- Branch: `{}`\n- Head: `{}`\n- Source: operator-selected canonical worktree\n<!-- /shea-symphony-workspace-adoption -->",
        issue.identifier,
        candidate.path.display(),
        candidate.branch.as_deref().unwrap_or("unknown"),
        candidate.head.as_deref().unwrap_or("unknown")
    );

    format!("{marker}\n{block}")
}

pub fn render_workspace_ensure_workpad(
    issue: &TrackerIssue,
    marker: &str,
    candidate: &IssueWorkspaceCandidate,
    action: &str,
    pr_ref: Option<&str>,
) -> String {
    let mut workpad = issue
        .description
        .as_deref()
        .and_then(|description| {
            description
                .find(marker)
                .map(|index| description[index..].trim())
        })
        .map(str::to_string)
        .unwrap_or_else(|| format!("{marker}\n## Shea Symphony Workpad"));

    let block = format!(
        "<!-- shea-symphony-workspace-ensure -->\n### Workspace Evidence\n- Issue: `{}`\n- Pull request: `{}`\n- Branch/ref: `{}`\n- Workspace path: `{}`\n- Action: `{}`\n- Source command: `workspace ensure`\n- Validation result: `clean local git worktree for Review/Merge inspection`\n<!-- /shea-symphony-workspace-ensure -->",
        issue.identifier,
        pr_ref.unwrap_or("none"),
        candidate.branch.as_deref().unwrap_or("unknown"),
        candidate.path.display(),
        action,
    );

    workpad = replace_or_append_block(
        &workpad,
        "<!-- shea-symphony-workspace-ensure -->",
        "<!-- /shea-symphony-workspace-ensure -->",
        &block,
    );
    workpad
}

fn upsert_candidate(
    candidates: &mut BTreeMap<PathBuf, IssueWorkspaceCandidate>,
    path: PathBuf,
    branch: Option<String>,
    head: Option<String>,
    strength: WorkspaceMatchStrength,
    evidence: WorkspaceEvidence,
) {
    let candidate = candidates
        .entry(path.clone())
        .or_insert_with(|| IssueWorkspaceCandidate {
            path,
            branch: branch.clone(),
            head: head.clone(),
            strength,
            evidence: Vec::new(),
        });
    if candidate.branch.is_none() {
        candidate.branch = branch;
    }
    if candidate.head.is_none() {
        candidate.head = head;
    }
    candidate.strength = candidate.strength.max(strength);
    if !candidate
        .evidence
        .iter()
        .any(|existing| existing == &evidence)
    {
        candidate.evidence.push(evidence);
    }
}

fn issue_workspace_hints(issue: &TrackerIssue) -> Vec<String> {
    let mut hints = BTreeSet::new();
    if let Some(branch) = issue.branch_name.as_deref() {
        hints.insert(branch.to_string());
    }
    for pr in &issue.linked_pull_requests {
        if let Some(branch) = pr.head_ref_name.as_deref() {
            hints.insert(branch.to_string());
        }
    }
    if let Some(number) = issue_number_token(&issue.identifier) {
        hints.insert(format!("feature/issue-{number}"));
        hints.insert(format!("issue-{number}"));
    }
    let slug = issue_slug(issue);
    if !slug.is_empty() {
        if let Some(number) = issue_number_token(&issue.identifier) {
            hints.insert(format!("feature/issue-{number}-{slug}"));
            hints.insert(format!("issue-{number}-{slug}"));
        }
    }
    hints.into_iter().collect()
}

fn issue_slug(issue: &TrackerIssue) -> String {
    issue
        .title
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join("-")
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn issue_number_token(issue_ref: &str) -> Option<String> {
    let number = issue_ref
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if number.is_empty() {
        None
    } else {
        Some(number)
    }
}

fn session_matches_issue(session: &AgentSessionRecord, issue_ref: &str) -> bool {
    let expected = issue_number_token(issue_ref);
    session
        .issue_identifier
        .as_deref()
        .is_some_and(|candidate| issue_number_token(candidate) == expected)
}

fn branch_matches_issue(issue: &TrackerIssue, branch: &str) -> bool {
    let hints = issue_workspace_hints(issue);
    hints.iter().any(|hint| branch == hint)
        || issue_number_token(&issue.identifier).is_some_and(|number| {
            branch.contains(&format!("issue-{number}")) || branch.ends_with(&number)
        })
}

fn workpad_workspace_hints(description: &str, marker: &str) -> Vec<PathBuf> {
    let Some(index) = description.find(marker) else {
        return Vec::new();
    };
    let workpad = &description[index..];
    let mut paths = BTreeSet::new();
    for line in workpad.lines() {
        if line.trim_start().starts_with("<!--") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if !is_workspace_path_hint_line(&lower) {
            continue;
        }
        for token in line.split(['`', ' ', '\t', ',', ')', '(']) {
            let trimmed = token.trim_matches([':', '-', '*']);
            if trimmed.starts_with('/') {
                paths.insert(PathBuf::from(trimmed));
            }
        }
    }
    paths.into_iter().collect()
}

fn is_workspace_path_hint_line(lower_line: &str) -> bool {
    lower_line.contains("workspace path:")
        || lower_line.contains("worktree path:")
        || lower_line.trim_start().starts_with("- path:")
        || lower_line.trim_start().starts_with("path:")
}

fn replace_or_append_block(content: &str, start: &str, end: &str, block: &str) -> String {
    if let Some(start_index) = content.find(start) {
        if let Some(end_offset) = content[start_index..].find(end) {
            let end_index = start_index + end_offset + end.len();
            return format!(
                "{}{}{}",
                content[..start_index].trim_end(),
                block,
                content[end_index..].trim_start()
            );
        }
    }
    format!("{}\n\n{}", content.trim_end(), block)
}

pub fn infer_issue_ref_from_branch_or_path(branch: Option<&str>, path: &Path) -> Option<String> {
    branch
        .and_then(issue_ref_from_text)
        .or_else(|| issue_ref_from_text(&path.to_string_lossy()))
}

fn issue_ref_from_text(text: &str) -> Option<String> {
    let marker = "issue-";
    let index = text.find(marker)?;
    let digits = text[index + marker.len()..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        Some(format!("#{digits}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LinkedPullRequest, TrackerIssue};
    use crate::session_registry::{AgentSessionRecord, SessionStatus};

    fn issue() -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "github_project_v2".into(),
            id: "I_253".into(),
            item_id: None,
            identifier: "#253".into(),
            title: "Share issue worktree discovery across Main Review and Merge lanes".into(),
            description: Some(
                "<!-- shea-symphony-workpad -->\n- Workspace: `/tmp/manual-issue-253`".into(),
            ),
            url: None,
            state: "In Progress".into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: None,
            linked_pull_requests: vec![LinkedPullRequest {
                head_ref_name: Some("feature/issue-253-worktree-discovery".into()),
                ..Default::default()
            }],
            blocked_by: Vec::new(),
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    fn session(path: &str) -> AgentSessionRecord {
        AgentSessionRecord {
            issue_id: Some("I_253".into()),
            issue_identifier: Some("#253".into()),
            issue_title: Some("Title".into()),
            lane: "main".into(),
            run_id: Some("run-253".into()),
            thread: None,
            session_source: None,
            claim_value: None,
            actor_role: Some("Main Agent".into()),
            actor_label: None,
            git_author: None,
            profile_id: None,
            instance_name: None,
            worktree: PathBuf::from(path),
            branch: Some("feature/issue-253-worktree-discovery".into()),
            backend: "tmux".into(),
            session_name: "shea-main-253".into(),
            pane_target: "shea:0.0".into(),
            prompt_artifact_path: PathBuf::from("/tmp/prompt"),
            log_path: PathBuf::from("/tmp/log"),
            attach_command: "tmux attach -t shea".into(),
            attempt: 1,
            status: SessionStatus::Running,
            started_at_ms: 1,
            updated_at_ms: 2,
        }
    }

    #[test]
    fn parses_git_worktree_porcelain_records() {
        let parsed = parse_git_worktree_porcelain(
            "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/issue-253\nHEAD def\nbranch refs/heads/feature/issue-253-demo\nprunable stale\n",
        );

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].path, PathBuf::from("/tmp/issue-253"));
        assert_eq!(parsed[1].branch.as_deref(), Some("feature/issue-253-demo"));
        assert_eq!(parsed[1].prunable.as_deref(), Some("stale"));
    }

    #[test]
    fn combines_session_workpad_pr_and_git_worktree_evidence() {
        let worktrees = vec![
            GitWorktree {
                path: PathBuf::from("/tmp/issue-253"),
                head: Some("def".into()),
                branch: Some("feature/issue-253-worktree-discovery".into()),
                prunable: None,
            },
            GitWorktree {
                path: PathBuf::from("/tmp/manual-issue-253"),
                head: Some("abc".into()),
                branch: Some("feature/issue-253-manual".into()),
                prunable: None,
            },
        ];
        let report = discover_issue_workspaces_from_parts(
            &issue(),
            &[session("/tmp/issue-253")],
            &worktrees,
            "<!-- shea-symphony-workpad -->",
        );

        assert_eq!(report.candidates.len(), 2);
        assert_eq!(report.canonical_index, None);
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("multiple strong")));
    }

    #[test]
    fn ignores_manual_claim_registry_records_as_workspace_candidates() {
        let mut manual = session("/repo");
        manual.backend = "codex-app-manual".into();
        manual.session_source = Some("manual-claim".into());
        manual.status = SessionStatus::Recorded;
        let mut issue = issue();
        issue.description = None;
        issue.linked_pull_requests = Vec::new();
        let report = discover_issue_workspaces_from_parts(
            &issue,
            &[manual],
            &[GitWorktree {
                path: PathBuf::from("/repo"),
                head: Some("def".into()),
                branch: Some("main".into()),
                prunable: None,
            }],
            "<!-- shea-symphony-workpad -->",
        );

        assert!(report.candidates.is_empty());
        assert_eq!(report.canonical_index, None);
    }

    #[test]
    fn workspace_discovery_tolerates_unrelated_unknown_persisted_status() {
        let mut unrelated = session("/tmp/issue-999");
        unrelated.issue_id = Some("I_999".into());
        unrelated.issue_identifier = Some("#999".into());
        unrelated.status = SessionStatus::UnknownPersisted("recorded_legacy".into());
        let report = discover_issue_workspaces_from_parts(
            &issue(),
            &[unrelated, session("/tmp/issue-253")],
            &[GitWorktree {
                path: PathBuf::from("/tmp/issue-253"),
                head: Some("def".into()),
                branch: Some("feature/issue-253-worktree-discovery".into()),
                prunable: None,
            }],
            "<!-- shea-symphony-workpad -->",
        );

        assert!(report
            .candidates
            .iter()
            .any(|candidate| candidate.path == Path::new("/tmp/issue-253")));
        assert!(!report
            .candidates
            .iter()
            .any(|candidate| candidate.path == Path::new("/tmp/issue-999")));
    }

    #[test]
    fn validates_adoption_against_repo_worktree_and_issue_branch() {
        let candidate = validate_workspace_adoption(
            &issue(),
            Path::new("/tmp/issue-253"),
            &[GitWorktree {
                path: PathBuf::from("/tmp/issue-253"),
                head: Some("def".into()),
                branch: Some("feature/issue-253-worktree-discovery".into()),
                prunable: None,
            }],
        )
        .unwrap();

        assert_eq!(
            candidate.branch.as_deref(),
            Some("feature/issue-253-worktree-discovery")
        );
    }

    #[test]
    fn renders_adoption_block_without_losing_workpad_marker() {
        let body = render_workspace_adoption_workpad(
            &issue(),
            "<!-- shea-symphony-workpad -->",
            &IssueWorkspaceCandidate {
                path: PathBuf::from("/tmp/issue-253"),
                branch: Some("feature/issue-253-worktree-discovery".into()),
                head: Some("def".into()),
                strength: WorkspaceMatchStrength::Strong,
                evidence: Vec::new(),
            },
        );

        assert!(body.starts_with("<!-- shea-symphony-workpad -->"));
        assert!(body.contains("Workspace Adoption"));
        assert!(body.contains("/tmp/issue-253"));
        assert!(!body.contains("/tmp/manual-issue-253"));
    }

    #[test]
    fn renders_ensure_block_as_workspace_evidence() {
        let body = render_workspace_ensure_workpad(
            &issue(),
            "<!-- shea-symphony-workpad -->",
            &IssueWorkspaceCandidate {
                path: PathBuf::from("/tmp/issue-253"),
                branch: Some("feature/issue-253-worktree-discovery".into()),
                head: Some("def".into()),
                strength: WorkspaceMatchStrength::Strong,
                evidence: Vec::new(),
            },
            "reused",
            Some("#254"),
        );

        assert!(body.starts_with("<!-- shea-symphony-workpad -->"));
        assert!(body.contains("Workspace Evidence"));
        assert!(body.contains("workspace ensure"));
        assert!(body.contains("Pull request: `#254`"));
    }

    #[test]
    fn workpad_hints_ignore_html_marker_lines() {
        let paths = workpad_workspace_hints(
            "<!-- shea-symphony-workpad -->\n<!-- shea-symphony-workspace-adoption -->\n- Path: `/tmp/issue-253`\n<!-- /shea-symphony-workspace-adoption -->",
            "<!-- shea-symphony-workpad -->",
        );

        assert_eq!(paths, vec![PathBuf::from("/tmp/issue-253")]);
    }

    #[test]
    fn workpad_hints_do_not_scan_plain_issue_body_paths() {
        let paths = workpad_workspace_hints(
            "- Reuse the existing `requestOperatorLocalArtifactsRefresh` / `REFRESH_REQUEST_EVENT` local-only path.\n- Open the `/lanes` route.",
            "<!-- shea-symphony-workpad -->",
        );

        assert!(paths.is_empty());
    }

    #[test]
    fn workpad_hints_only_accept_explicit_workspace_path_lines() {
        let paths = workpad_workspace_hints(
            "<!-- shea-symphony-workpad -->\n- Reuse the existing `requestOperatorLocalArtifactsRefresh` / `REFRESH_REQUEST_EVENT` local-only path.\n- Workspace path: `/tmp/issue-253`",
            "<!-- shea-symphony-workpad -->",
        );

        assert_eq!(paths, vec![PathBuf::from("/tmp/issue-253")]);
    }
}
