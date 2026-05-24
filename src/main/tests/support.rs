use super::*;

pub(crate) fn forge_contract() -> String {
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

pub(crate) fn parse(args: &[&str]) -> Command {
    Command::parse(args.iter().map(|arg| arg.to_string()).collect()).unwrap()
}

pub(crate) fn git_ok(path: &Path, args: &[&str]) {
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

pub(crate) fn canonical_git_repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
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

pub(crate) fn help_text(args: &[&str]) -> String {
    let Command::Help(text) = parse(args) else {
        panic!("expected help command");
    };
    text
}

pub(crate) fn test_config() -> RuntimeConfig {
    let workflow = WorkflowDefinition::parse(
        "/tmp/WORKFLOW.md",
        "---\ntracker:\n  kind: memory\n---\nPrompt",
    )
    .unwrap();
    RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
}

pub(crate) fn main_loop_test_config() -> RuntimeConfig {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\n  active_states:\n    - Todo\n    - Rework\n  terminal_states:\n    - Done\n---\nPrompt",
        )
        .unwrap();
    RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
}

pub(crate) fn live_github_config(allow_unassigned: bool) -> RuntimeConfig {
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

pub(crate) fn fixture_github_config() -> RuntimeConfig {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\n  fixture_path: fixtures/dry-run-issues.json\n---\nPrompt",
        )
        .unwrap();
    RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
}

pub(crate) fn tracker_issue(state: &str) -> TrackerIssue {
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

pub(crate) fn test_claim(issue: &TrackerIssue) -> LaneClaim {
    LaneClaim::active(
        &issue.identifier,
        LaneClaimLane::Main,
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        1_779_000_900_123,
    )
}

pub(crate) fn tracker_issue_with_ref(identifier: &str, title: &str, state: &str) -> TrackerIssue {
    let mut issue = tracker_issue(state);
    issue.identifier = identifier.into();
    issue.title = title.into();
    issue.branch_name = None;
    issue
}

pub(crate) struct RecordingAdapter {
    pub(crate) operations: RefCell<Vec<String>>,
    pub(crate) issues: RefCell<BTreeMap<String, TrackerIssue>>,
    pub(crate) hydrated_issues: RefCell<Vec<String>>,
    pub(crate) linked_pull_requests: RefCell<Vec<jade_symphony::model::LinkedPullRequest>>,
    pub(crate) fail_workpad: bool,
    pub(crate) fail_comment: bool,
    pub(crate) fail_link_pr: bool,
    pub(crate) fail_state_after_apply: bool,
    pub(crate) fail_project_field_after_apply: bool,
    pub(crate) fail_workpad_after_apply: bool,
    pub(crate) fail_comment_after_apply: bool,
    pub(crate) fail_close_after_apply: bool,
    pub(crate) confirm_link_pr: bool,
    pub(crate) fail_get_issue: bool,
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
    pub(crate) fn operations(&self) -> Vec<String> {
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
                jade_symphony::tracker::TrackerError::IntegrationUnavailable("link failed".into()),
            );
        }
        self.operations
            .borrow_mut()
            .push(format!("link_pr:{issue_ref}:{pr_ref}"));
        if self.confirm_link_pr {
            self.linked_pull_requests
                .borrow_mut()
                .push(jade_symphony::model::LinkedPullRequest {
                    number: pull_request_number_from_url(pr_ref),
                    url: Some(pr_ref.to_string()),
                    state: Some("OPEN".into()),
                    is_draft: Some(false),
                    ..Default::default()
                });
        }
        Ok(())
    }

    fn list_linked_pull_requests(
        &self,
        _issue_ref: &str,
    ) -> Result<Vec<jade_symphony::model::LinkedPullRequest>, jade_symphony::tracker::TrackerError>
    {
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

pub(crate) struct MergeRecoveryRunner {
    pub(crate) calls: RefCell<Vec<String>>,
}

impl MergeRecoveryRunner {
    pub(crate) fn new() -> Self {
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
