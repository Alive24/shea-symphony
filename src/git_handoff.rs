use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::handoff::IssueHandoffPlan;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWorktreeResult {
    pub workspace_path: PathBuf,
    pub branch_name: String,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestPublication {
    pub branch_pushed: bool,
    pub pr_url: String,
    pub pr_created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestReadyStatus {
    pub pr_url: String,
    pub was_draft: bool,
    pub marked_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait HandoffCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<CommandOutput, GitHandoffError>;
}

#[derive(Debug, Default)]
pub struct ProcessHandoffCommandRunner;

impl HandoffCommandRunner for ProcessHandoffCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<CommandOutput, GitHandoffError> {
        let output = Command::new(program).args(args).current_dir(cwd).output()?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

#[derive(Debug, Error)]
pub enum GitHandoffError {
    #[error("git handoff io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{program} failed with status {status}: stdout={stdout} stderr={stderr}")]
    CommandFailed {
        program: String,
        status: i32,
        stdout: String,
        stderr: String,
    },
    #[error("existing worktree {path} is on branch {current_branch}, expected {expected_branch}")]
    WorktreeBranchMismatch {
        path: PathBuf,
        expected_branch: String,
        current_branch: String,
    },
    #[error("pull request command did not return a URL")]
    MissingPullRequestUrl,
    #[error("pull request view command returned invalid JSON: {0}")]
    InvalidPullRequestViewPayload(String),
    #[error("pull request ready command returned invalid JSON: {0}")]
    InvalidPullRequestReadyPayload(String),
    #[error("worktree {path} has uncommitted changes before PR handoff: {status}")]
    DirtyWorktree { path: PathBuf, status: String },
    #[error("branch {branch} has no commits ahead of base {base} before PR handoff")]
    NoCommitsAhead { branch: String, base: String },
    #[error("git rev-list returned an invalid ahead count: {value}")]
    InvalidAheadCount { value: String },
}

pub fn prepare_issue_worktree(
    repo_root: &Path,
    plan: &IssueHandoffPlan,
    runner: &dyn HandoffCommandRunner,
) -> Result<LiveWorktreeResult, GitHandoffError> {
    if plan.workspace_path.exists() {
        let current_branch = current_branch(&plan.workspace_path, runner)?;
        if current_branch != plan.branch_name {
            return Err(GitHandoffError::WorktreeBranchMismatch {
                path: plan.workspace_path.clone(),
                expected_branch: plan.branch_name.clone(),
                current_branch,
            });
        }

        return Ok(LiveWorktreeResult {
            workspace_path: plan.workspace_path.clone(),
            branch_name: plan.branch_name.clone(),
            created: false,
        });
    }

    if let Some(parent) = plan.workspace_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let branch_ref = format!("refs/heads/{}", plan.branch_name);
    let branch_exists = command_status(
        "git",
        &[
            "-C".into(),
            repo_root.display().to_string(),
            "show-ref".into(),
            "--verify".into(),
            "--quiet".into(),
            branch_ref,
        ],
        repo_root,
        runner,
    )? == 0;

    let args = if branch_exists {
        vec![
            "-C".into(),
            repo_root.display().to_string(),
            "worktree".into(),
            "add".into(),
            plan.workspace_path.display().to_string(),
            plan.branch_name.clone(),
        ]
    } else {
        vec![
            "-C".into(),
            repo_root.display().to_string(),
            "worktree".into(),
            "add".into(),
            "-b".into(),
            plan.branch_name.clone(),
            plan.workspace_path.display().to_string(),
            plan.pull_request.base_branch.clone(),
        ]
    };
    require_success("git", runner.run("git", &args, repo_root)?)?;

    Ok(LiveWorktreeResult {
        workspace_path: plan.workspace_path.clone(),
        branch_name: plan.branch_name.clone(),
        created: true,
    })
}

pub fn publish_issue_pull_request(
    plan: &IssueHandoffPlan,
    runner: &dyn HandoffCommandRunner,
) -> Result<PullRequestPublication, GitHandoffError> {
    ensure_publishable_branch(plan, runner)?;

    require_success(
        "git",
        runner.run(
            "git",
            &[
                "push".into(),
                "-u".into(),
                "origin".into(),
                plan.branch_name.clone(),
            ],
            &plan.workspace_path,
        )?,
    )?;

    let existing = runner.run(
        "gh",
        &[
            "pr".into(),
            "view".into(),
            plan.branch_name.clone(),
            "--json".into(),
            "url,body".into(),
        ],
        &plan.workspace_path,
    )?;
    if existing.status == 0 {
        let view = parse_pull_request_view(&existing.stdout)?;
        ensure_existing_pull_request_body_links_issue(plan, &view.body, runner)?;
        return Ok(PullRequestPublication {
            branch_pushed: true,
            pr_url: view.url,
            pr_created: false,
        });
    }

    let created = runner.run(
        "gh",
        &[
            "pr".into(),
            "create".into(),
            "--title".into(),
            plan.pull_request.title.clone(),
            "--body".into(),
            plan.pull_request.body.clone(),
            "--base".into(),
            plan.pull_request.base_branch.clone(),
            "--head".into(),
            plan.pull_request.head_branch.clone(),
        ],
        &plan.workspace_path,
    )?;
    require_success("gh", created.clone())?;

    Ok(PullRequestPublication {
        branch_pushed: true,
        pr_url: extract_url(&created.stdout)?,
        pr_created: true,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PullRequestView {
    url: String,
    body: String,
}

fn parse_pull_request_view(stdout: &str) -> Result<PullRequestView, GitHandoffError> {
    let value: serde_json::Value = serde_json::from_str(stdout)
        .map_err(|error| GitHandoffError::InvalidPullRequestViewPayload(error.to_string()))?;
    let url = value
        .get("url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .ok_or(GitHandoffError::MissingPullRequestUrl)?
        .to_string();
    let body = value
        .get("body")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(PullRequestView { url, body })
}

fn ensure_existing_pull_request_body_links_issue(
    plan: &IssueHandoffPlan,
    existing_body: &str,
    runner: &dyn HandoffCommandRunner,
) -> Result<(), GitHandoffError> {
    if pull_request_body_has_closing_issue_reference(existing_body, &plan.issue_ref) {
        return Ok(());
    }

    let updated_body = append_issue_closing_reference(existing_body, &plan.issue_ref);
    require_success(
        "gh",
        runner.run(
            "gh",
            &[
                "pr".into(),
                "edit".into(),
                plan.branch_name.clone(),
                "--body".into(),
                updated_body,
            ],
            &plan.workspace_path,
        )?,
    )
}

fn pull_request_body_has_closing_issue_reference(body: &str, issue_ref: &str) -> bool {
    let issue_ref = issue_ref.trim().to_ascii_lowercase();
    if issue_ref.is_empty() {
        return false;
    }
    body.lines().any(|line| {
        let line = line.to_ascii_lowercase();
        let has_closing_keyword = line
            .split(|character: char| !character.is_ascii_alphabetic())
            .any(|token| {
                matches!(
                    token,
                    "close"
                        | "closes"
                        | "closed"
                        | "fix"
                        | "fixes"
                        | "fixed"
                        | "resolve"
                        | "resolves"
                        | "resolved"
                )
            });
        has_closing_keyword && line_contains_issue_ref(&line, &issue_ref)
    })
}

fn line_contains_issue_ref(line: &str, issue_ref: &str) -> bool {
    let mut rest = line;
    while let Some(index) = rest.find(issue_ref) {
        let after_index = index + issue_ref.len();
        let after = rest[after_index..].chars().next();
        if !after
            .map(|character| character.is_ascii_digit())
            .unwrap_or(false)
        {
            return true;
        }
        rest = &rest[after_index..];
    }
    false
}

fn append_issue_closing_reference(body: &str, issue_ref: &str) -> String {
    let body = body.trim_end();
    if body.is_empty() {
        return format!("Closes {}\n", issue_ref.trim());
    }
    format!("{body}\n\n## Issue Link\n\nCloses {}\n", issue_ref.trim())
}

pub fn ensure_pull_request_ready(
    pr_ref: &str,
    runner: &dyn HandoffCommandRunner,
    cwd: &Path,
) -> Result<PullRequestReadyStatus, GitHandoffError> {
    let viewed = runner.run(
        "gh",
        &[
            "pr".into(),
            "view".into(),
            pr_ref.into(),
            "--json".into(),
            "url,isDraft".into(),
        ],
        cwd,
    )?;
    require_success("gh", viewed.clone())?;
    let value: serde_json::Value = serde_json::from_str(&viewed.stdout)
        .map_err(|error| GitHandoffError::InvalidPullRequestReadyPayload(error.to_string()))?;
    let pr_url = value
        .get("url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| GitHandoffError::InvalidPullRequestReadyPayload("missing url".into()))?
        .to_string();
    let was_draft = value
        .get("isDraft")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| GitHandoffError::InvalidPullRequestReadyPayload("missing isDraft".into()))?;

    if was_draft {
        let ready = runner.run("gh", &["pr".into(), "ready".into(), pr_ref.into()], cwd)?;
        require_success("gh", ready)?;
    }

    Ok(PullRequestReadyStatus {
        pr_url,
        was_draft,
        marked_ready: was_draft,
    })
}

fn ensure_publishable_branch(
    plan: &IssueHandoffPlan,
    runner: &dyn HandoffCommandRunner,
) -> Result<(), GitHandoffError> {
    let status = runner.run(
        "git",
        &["status".into(), "--porcelain".into()],
        &plan.workspace_path,
    )?;
    require_success("git", status.clone())?;
    if !status.stdout.trim().is_empty() {
        return Err(GitHandoffError::DirtyWorktree {
            path: plan.workspace_path.clone(),
            status: compact_git_evidence(&status.stdout),
        });
    }

    let range = format!("{}..HEAD", plan.pull_request.base_branch);
    let ahead = runner.run(
        "git",
        &["rev-list".into(), "--count".into(), range],
        &plan.workspace_path,
    )?;
    require_success("git", ahead.clone())?;
    let count =
        ahead
            .stdout
            .trim()
            .parse::<u32>()
            .map_err(|_| GitHandoffError::InvalidAheadCount {
                value: ahead.stdout.trim().to_string(),
            })?;
    if count == 0 {
        return Err(GitHandoffError::NoCommitsAhead {
            branch: plan.branch_name.clone(),
            base: plan.pull_request.base_branch.clone(),
        });
    }

    Ok(())
}

fn current_branch(
    workspace_path: &Path,
    runner: &dyn HandoffCommandRunner,
) -> Result<String, GitHandoffError> {
    let output = runner.run(
        "git",
        &["branch".into(), "--show-current".into()],
        workspace_path,
    )?;
    require_success("git", output.clone())?;
    Ok(output.stdout.trim().to_string())
}

fn command_status(
    program: &str,
    args: &[String],
    cwd: &Path,
    runner: &dyn HandoffCommandRunner,
) -> Result<i32, GitHandoffError> {
    Ok(runner.run(program, args, cwd)?.status)
}

fn require_success(program: &str, output: CommandOutput) -> Result<(), GitHandoffError> {
    if output.status == 0 {
        Ok(())
    } else {
        Err(GitHandoffError::CommandFailed {
            program: program.into(),
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

fn extract_url(stdout: &str) -> Result<String, GitHandoffError> {
    stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("http://") || line.starts_with("https://"))
        .map(ToOwned::to_owned)
        .ok_or(GitHandoffError::MissingPullRequestUrl)
}

fn compact_git_evidence(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 240;
    let truncated = compact.chars().take(LIMIT).collect::<String>();
    if truncated.len() < compact.len() {
        format!("{truncated}...")
    } else {
        compact
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handoff::plan_issue_handoff;
    use crate::model::TrackerIssue;
    use std::cell::RefCell;

    #[derive(Debug, Default)]
    struct FakeRunner {
        commands: RefCell<Vec<String>>,
        existing_branch: bool,
        existing_pr_url: Option<String>,
        existing_pr_body: String,
        worktree_branch: Option<String>,
        dirty_status: Option<String>,
        ahead_count: u32,
        pr_is_draft: bool,
    }

    impl FakeRunner {
        fn clean_with_commits() -> Self {
            Self {
                ahead_count: 1,
                ..Default::default()
            }
        }
    }

    impl HandoffCommandRunner for FakeRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
            _cwd: &Path,
        ) -> Result<CommandOutput, GitHandoffError> {
            let command = format!("{program} {}", args.join(" "));
            self.commands.borrow_mut().push(command.clone());

            if command.contains("show-ref --verify --quiet") {
                return Ok(CommandOutput {
                    status: if self.existing_branch { 0 } else { 1 },
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            if command.contains("branch --show-current") {
                return Ok(CommandOutput {
                    status: 0,
                    stdout: self.worktree_branch.clone().unwrap_or_default(),
                    stderr: String::new(),
                });
            }
            if command.contains("status --porcelain") {
                return Ok(CommandOutput {
                    status: 0,
                    stdout: self.dirty_status.clone().unwrap_or_default(),
                    stderr: String::new(),
                });
            }
            if command.contains("rev-list --count") {
                return Ok(CommandOutput {
                    status: 0,
                    stdout: format!("{}\n", self.ahead_count),
                    stderr: String::new(),
                });
            }
            if command.starts_with("gh pr view") {
                return Ok(match &self.existing_pr_url {
                    Some(url) => CommandOutput {
                        status: 0,
                        stdout: if command.contains("url,isDraft") {
                            format!("{{\"url\":\"{}\",\"isDraft\":{}}}\n", url, self.pr_is_draft)
                        } else if command.contains("url,body") {
                            serde_json::json!({
                                "url": url,
                                "body": self.existing_pr_body,
                            })
                            .to_string()
                        } else {
                            format!("{url}\n")
                        },
                        stderr: String::new(),
                    },
                    None => CommandOutput {
                        status: 1,
                        stdout: String::new(),
                        stderr: "no pull request".into(),
                    },
                });
            }
            if command.starts_with("gh pr create") {
                return Ok(CommandOutput {
                    status: 0,
                    stdout: "https://github.com/Alive24/jade-symphony/pull/99\n".into(),
                    stderr: String::new(),
                });
            }
            if command.starts_with("gh pr ready") {
                return Ok(CommandOutput {
                    status: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            if command.starts_with("gh pr edit") {
                return Ok(CommandOutput {
                    status: 0,
                    stdout: String::new(),
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

    fn issue() -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "github_project_v2".into(),
            id: "I_45".into(),
            item_id: Some("PVTI_45".into()),
            identifier: "#45".into(),
            title: "Wire live worktree and PR creation into main loop".into(),
            description: None,
            url: None,
            state: "In Progress".into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: None,
            linked_pull_requests: Vec::new(),
            blocked_by: Vec::new(),
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn creates_new_issue_worktree_from_base_branch() {
        let temp = tempfile::tempdir().unwrap();
        let plan = plan_issue_handoff(temp.path(), &issue(), "main").unwrap();
        let runner = FakeRunner::default();

        let result = prepare_issue_worktree(temp.path(), &plan, &runner).unwrap();

        assert!(result.created);
        assert_eq!(result.branch_name, plan.branch_name);
        let commands = runner.commands.borrow().join("\n");
        assert!(commands.contains("show-ref --verify --quiet"));
        assert!(commands.contains("worktree add -b feature/issue-45"));
    }

    #[test]
    fn refuses_existing_worktree_on_different_branch() {
        let temp = tempfile::tempdir().unwrap();
        let workspace_root = temp.path().join("workspaces");
        let plan = plan_issue_handoff(&workspace_root, &issue(), "main").unwrap();
        fs::create_dir_all(&plan.workspace_path).unwrap();
        let runner = FakeRunner {
            worktree_branch: Some("feature/issue-99-other".into()),
            ..Default::default()
        };

        let error = prepare_issue_worktree(temp.path(), &plan, &runner).unwrap_err();

        assert!(matches!(
            error,
            GitHandoffError::WorktreeBranchMismatch { .. }
        ));
    }

    #[test]
    fn reuses_existing_pull_request_after_push() {
        let temp = tempfile::tempdir().unwrap();
        let plan = plan_issue_handoff(temp.path(), &issue(), "main").unwrap();
        let runner = FakeRunner {
            ahead_count: 1,
            existing_pr_url: Some("https://github.com/Alive24/jade-symphony/pull/45".into()),
            existing_pr_body: "Closes #45".into(),
            ..Default::default()
        };

        let result = publish_issue_pull_request(&plan, &runner).unwrap();

        assert!(result.branch_pushed);
        assert!(!result.pr_created);
        assert_eq!(
            result.pr_url,
            "https://github.com/Alive24/jade-symphony/pull/45"
        );
        let commands = runner.commands.borrow().join("\n");
        assert!(commands.contains("git push -u origin"));
        assert!(!commands.contains("gh pr edit"));
        assert!(!commands.contains("gh pr create"));
    }

    #[test]
    fn repairs_existing_pull_request_body_without_issue_closing_reference() {
        let temp = tempfile::tempdir().unwrap();
        let plan = plan_issue_handoff(temp.path(), &issue(), "main").unwrap();
        let runner = FakeRunner {
            ahead_count: 1,
            existing_pr_url: Some("https://github.com/Alive24/jade-symphony/pull/45".into()),
            existing_pr_body: "## Summary\n\nRecovered implementation.".into(),
            ..Default::default()
        };

        let result = publish_issue_pull_request(&plan, &runner).unwrap();

        assert!(result.branch_pushed);
        assert!(!result.pr_created);
        assert_eq!(
            result.pr_url,
            "https://github.com/Alive24/jade-symphony/pull/45"
        );
        let commands = runner.commands.borrow().join("\n");
        assert!(commands.contains("gh pr edit"));
        assert!(commands.contains("## Issue Link"));
        assert!(commands.contains("Closes #45"));
        assert!(!commands.contains("gh pr create"));
    }

    #[test]
    fn closing_issue_reference_requires_exact_issue_number() {
        assert!(pull_request_body_has_closing_issue_reference(
            "Resolves #45.",
            "#45"
        ));
        assert!(!pull_request_body_has_closing_issue_reference(
            "Resolves #450.",
            "#45"
        ));
        assert!(!pull_request_body_has_closing_issue_reference(
            "Mentioned #45 without a closing keyword.",
            "#45"
        ));
    }

    #[test]
    fn creates_pull_request_when_none_exists() {
        let temp = tempfile::tempdir().unwrap();
        let plan = plan_issue_handoff(temp.path(), &issue(), "main").unwrap();
        let runner = FakeRunner::clean_with_commits();

        let result = publish_issue_pull_request(&plan, &runner).unwrap();

        assert!(result.branch_pushed);
        assert!(result.pr_created);
        assert_eq!(
            result.pr_url,
            "https://github.com/Alive24/jade-symphony/pull/99"
        );
        let commands = runner.commands.borrow().join("\n");
        assert!(commands.contains("git status --porcelain"));
        assert!(commands.contains("git rev-list --count main..HEAD"));
        assert!(commands.contains("gh pr create"));
    }

    #[test]
    fn blocks_dirty_worktree_before_push() {
        let temp = tempfile::tempdir().unwrap();
        let plan = plan_issue_handoff(temp.path(), &issue(), "main").unwrap();
        let runner = FakeRunner {
            dirty_status: Some(" M src/main.rs\n".into()),
            ahead_count: 1,
            ..Default::default()
        };

        let error = publish_issue_pull_request(&plan, &runner).unwrap_err();

        assert!(matches!(error, GitHandoffError::DirtyWorktree { .. }));
        let commands = runner.commands.borrow().join("\n");
        assert!(commands.contains("git status --porcelain"));
        assert!(!commands.contains("git push -u origin"));
    }

    #[test]
    fn blocks_noop_branch_before_push() {
        let temp = tempfile::tempdir().unwrap();
        let plan = plan_issue_handoff(temp.path(), &issue(), "main").unwrap();
        let runner = FakeRunner {
            ahead_count: 0,
            ..Default::default()
        };

        let error = publish_issue_pull_request(&plan, &runner).unwrap_err();

        assert!(matches!(error, GitHandoffError::NoCommitsAhead { .. }));
        let commands = runner.commands.borrow().join("\n");
        assert!(commands.contains("git rev-list --count main..HEAD"));
        assert!(!commands.contains("git push -u origin"));
    }

    #[test]
    fn marks_existing_draft_pull_request_ready() {
        let temp = tempfile::tempdir().unwrap();
        let runner = FakeRunner {
            existing_pr_url: Some("https://github.com/Alive24/jade-symphony/pull/45".into()),
            pr_is_draft: true,
            ..Default::default()
        };

        let status = ensure_pull_request_ready(
            "https://github.com/Alive24/jade-symphony/pull/45",
            &runner,
            temp.path(),
        )
        .unwrap();

        assert!(status.was_draft);
        assert!(status.marked_ready);
        let commands = runner.commands.borrow().join("\n");
        assert!(commands.contains("gh pr view"));
        assert!(commands.contains("gh pr ready"));
    }

    #[test]
    fn leaves_ready_pull_request_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let runner = FakeRunner {
            existing_pr_url: Some("https://github.com/Alive24/jade-symphony/pull/45".into()),
            pr_is_draft: false,
            ..Default::default()
        };

        let status = ensure_pull_request_ready(
            "https://github.com/Alive24/jade-symphony/pull/45",
            &runner,
            temp.path(),
        )
        .unwrap();

        assert!(!status.was_draft);
        assert!(!status.marked_ready);
        let commands = runner.commands.borrow().join("\n");
        assert!(commands.contains("gh pr view"));
        assert!(!commands.contains("gh pr ready"));
    }
}
