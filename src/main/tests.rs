use super::*;
use jade_symphony::tracker::MemoryTracker;
use std::cell::RefCell;

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

fn forge_contract() -> String {
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

fn parse(args: &[&str]) -> Command {
    Command::parse(args.iter().map(|arg| arg.to_string()).collect()).unwrap()
}

fn git_ok(path: &Path, args: &[&str]) {
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

fn canonical_git_repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
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
    git_ok(&other, &["config", "user.email", "jade@example.invalid"]);
    git_ok(&other, &["config", "user.name", "Jade Symphony"]);
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
    config.identity.actor_label = "Jade Symphony Merge Worker".into();

    append_tracker_mutation_audit(
        &config,
        TrackerMutationAudit {
            command: "merge once",
            mutation_type: "state_change",
            issue_ref: Some("#7"),
            target: Some("https://github.com/Alive24/jade-symphony/pull/7".into()),
            from_state: Some("Merging".into()),
            to_state: Some("Done".into()),
            reason: "merge completed",
        },
    );

    let records = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"))
        .read_records()
        .unwrap();
    let record = records.first().expect("expected audit record");
    assert_eq!(record.event, "tracker_mutation");
    assert_eq!(record.actor_role.as_deref(), Some("merge_agent"));
    assert_eq!(
        record.actor_label.as_deref(),
        Some("Jade Symphony Merge Worker")
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
        .join("JADE_SYMPHONY_PROMPT.md")
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

    let records = EventLog::new(logs_root.join("jade-symphony.jsonl"))
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
        temporary_workflow_warning(Path::new("/private/tmp/jade-github-project-workflow.md"))
            .expect("expected temporary workflow warning");

    assert!(warning.contains("workflow_warning=temporary_path"));
    assert!(warning.contains("action=promote"));
    assert!(temporary_workflow_warning(Path::new("examples/github-project-workflow.md")).is_none());
}

fn help_text(args: &[&str]) -> String {
    let Command::Help(text) = parse(args) else {
        panic!("expected help command");
    };
    text
}

fn test_config() -> RuntimeConfig {
    let workflow = WorkflowDefinition::parse(
        "/tmp/WORKFLOW.md",
        "---\ntracker:\n  kind: memory\n---\nPrompt",
    )
    .unwrap();
    RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
}

fn main_loop_test_config() -> RuntimeConfig {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\n  active_states:\n    - Todo\n    - Rework\n  terminal_states:\n    - Done\n---\nPrompt",
        )
        .unwrap();
    RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
}

fn live_github_config(allow_unassigned: bool) -> RuntimeConfig {
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

fn fixture_github_config() -> RuntimeConfig {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\n  fixture_path: fixtures/dry-run-issues.json\n---\nPrompt",
        )
        .unwrap();
    RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
}

fn tracker_issue(state: &str) -> TrackerIssue {
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

fn test_claim(issue: &TrackerIssue) -> LaneClaim {
    LaneClaim::active(
        &issue.identifier,
        LaneClaimLane::Main,
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        1_779_000_900_123,
    )
}

fn tracker_issue_with_ref(identifier: &str, title: &str, state: &str) -> TrackerIssue {
    let mut issue = tracker_issue(state);
    issue.identifier = identifier.into();
    issue.title = title.into();
    issue.branch_name = None;
    issue
}

struct RecordingAdapter {
    operations: RefCell<Vec<String>>,
    issues: RefCell<BTreeMap<String, TrackerIssue>>,
    hydrated_issues: RefCell<Vec<String>>,
    linked_pull_requests: RefCell<Vec<jade_symphony::model::LinkedPullRequest>>,
    fail_workpad: bool,
    fail_comment: bool,
    fail_link_pr: bool,
    fail_state_after_apply: bool,
    fail_project_field_after_apply: bool,
    fail_workpad_after_apply: bool,
    fail_comment_after_apply: bool,
    fail_close_after_apply: bool,
    confirm_link_pr: bool,
    fail_get_issue: bool,
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
    fn operations(&self) -> Vec<String> {
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

struct MergeRecoveryRunner {
    calls: RefCell<Vec<String>>,
}

impl MergeRecoveryRunner {
    fn new() -> Self {
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
    .with_worker("Jade Symphony Agent");
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
    let workpad = "## Jade Symphony Merge Run\n\n- Result: `merged_or_done`";

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
        "https://github.com/Alive24/jade-symphony/pull/351",
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
fn renders_plan_snapshot_as_json_when_requested() {
    let snapshot = jade_symphony::model::RuntimeSnapshot {
        event_log_path: Some("/tmp/jade-symphony.jsonl".into()),
        integration_gaps: vec!["gap".into()],
        ..Default::default()
    };

    let rendered = render_plan_snapshot(&snapshot, true).unwrap();
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(
        value
            .get("event_log_path")
            .and_then(serde_json::Value::as_str),
        Some("/tmp/jade-symphony.jsonl")
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
