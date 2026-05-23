use super::*;
use jade_symphony::tracker::MemoryTracker;
use std::cell::RefCell;

#[path = "tests/parser.rs"]
mod parser;

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

fn clean_autopilot_canonical() -> AutopilotCanonicalCheckout {
    AutopilotCanonicalCheckout {
            safe_for_write: true,
            root: Some("/repo".into()),
            branch: Some("main".into()),
            upstream: Some("origin/main".into()),
            clean: Some(true),
            reason: None,
            status_line: Some("canonical_checkout root=/repo branch=main upstream=origin/main clean=true tracked_dirty=0 untracked=0 unclassified=0 migrated=0 quarantine=/repo/.tmp".into()),
        }
}

fn clean_autopilot_runtime() -> AutopilotRuntimeSummary {
    AutopilotRuntimeSummary {
        runtime_state_count: 0,
        session_count: 0,
        session_attention_count: 0,
        blockers: Vec::new(),
        evidence: Vec::new(),
    }
}

fn clean_autopilot_doctor(total_issues: usize) -> ProjectAuditReport {
    ProjectAuditReport {
        total_issues,
        violations: Vec::new(),
        integration_gaps: Vec::new(),
        skill_readiness_summary: None,
    }
}

fn test_autopilot_plan(issues: Vec<TrackerIssue>) -> AutopilotPlanSnapshot {
    let config = test_config();
    let adapter = jade_symphony::tracker::MemoryTracker::new(issues.clone());
    build_autopilot_plan_from_parts(AutopilotPlanInputs {
        workflow_path: Path::new("/tmp/WORKFLOW.md"),
        config: &config,
        adapter: &adapter,
        issues: issues.clone(),
        doctor_report: clean_autopilot_doctor(issues.len()),
        canonical_checkout: clean_autopilot_canonical(),
        runtime: clean_autopilot_runtime(),
        integration_gaps: Vec::new(),
    })
    .unwrap()
}

#[test]
fn autopilot_plan_reports_all_lanes_idle() {
    let plan = test_autopilot_plan(Vec::new());

    assert_eq!(plan.readiness.status, "idle_but_healthy");
    assert_eq!(
        plan.lanes
            .iter()
            .map(|lane| (lane.lane.as_str(), lane.reason.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("main", "no_dispatchable_issue"),
            ("review", "no_agent_review_issue"),
            ("merge", "no_merging_issue")
        ]
    );
    assert!(plan.lanes.iter().all(|lane| lane.selected_issue.is_none()));
}

#[test]
fn autopilot_plan_reports_merge_ready_issue() {
    let mut issue = tracker_issue_with_ref("#338", "Ready merge", "Merging");
    issue
        .linked_pull_requests
        .push(jade_symphony::model::LinkedPullRequest {
            number: Some(339),
            url: Some("https://github.com/Alive24/jade-symphony/pull/339".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            merge_state_status: Some("CLEAN".into()),
            review_decision: Some("APPROVED".into()),
            base_ref_name: Some("main".into()),
            head_ref_name: Some("feature/issue-338".into()),
            ..Default::default()
        });

    let plan = test_autopilot_plan(vec![issue]);
    let merge = plan.lanes.iter().find(|lane| lane.lane == "merge").unwrap();

    assert_eq!(plan.readiness.status, "ready");
    assert_eq!(merge.status, "ready");
    assert_eq!(merge.proposed_action, "merge_pull_request");
    assert_eq!(merge.target_state.as_deref(), Some("done"));
    assert_eq!(
        merge
            .selected_issue
            .as_ref()
            .map(|issue| issue.identifier.as_str()),
        Some("#338")
    );
}

#[test]
fn autopilot_plan_does_not_mutate_tracker_adapter() {
    let config = test_config();
    let issue = tracker_issue_with_ref("#338", "Ready merge", "Merging");
    let adapter = RecordingAdapter::default();
    adapter
        .linked_pull_requests
        .borrow_mut()
        .push(jade_symphony::model::LinkedPullRequest {
            number: Some(339),
            url: Some("https://github.com/Alive24/jade-symphony/pull/339".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            merge_state_status: Some("CLEAN".into()),
            review_decision: Some("APPROVED".into()),
            base_ref_name: Some("main".into()),
            head_ref_name: Some("feature/issue-338".into()),
            ..Default::default()
        });

    let plan = build_autopilot_plan_from_parts(AutopilotPlanInputs {
        workflow_path: Path::new("/tmp/WORKFLOW.md"),
        config: &config,
        adapter: &adapter,
        issues: vec![issue],
        doctor_report: clean_autopilot_doctor(1),
        canonical_checkout: clean_autopilot_canonical(),
        runtime: clean_autopilot_runtime(),
        integration_gaps: Vec::new(),
    })
    .unwrap();

    assert!(plan.read_only);
    assert_eq!(plan.readiness.status, "ready");
    assert!(adapter.operations().is_empty());
}

#[test]
fn autopilot_plan_reports_parked_operator_queues() {
    let human_review = tracker_issue_with_ref("#41", "Needs human approval", "Human Review");
    let need_human_input =
        tracker_issue_with_ref("#42", "Needs operator decision", "Need Human Input");

    let plan = test_autopilot_plan(vec![human_review, need_human_input]);

    let human_queue = plan
        .parked_queues
        .iter()
        .find(|queue| queue.name == "Human Review")
        .unwrap();
    let input_queue = plan
        .parked_queues
        .iter()
        .find(|queue| queue.name == "Need Human Input")
        .unwrap();
    assert_eq!(human_queue.count, 1);
    assert_eq!(input_queue.count, 1);
    assert!(plan.lanes.iter().all(|lane| lane.selected_issue.is_none()));
}

#[test]
fn autopilot_plan_blocks_on_doctor_or_canonical_checkout() {
    let config = test_config();
    let issues = Vec::new();
    let adapter = jade_symphony::tracker::MemoryTracker::new(issues.clone());
    let doctor = ProjectAuditReport {
        total_issues: 0,
        violations: vec![ProjectAuditViolation {
            issue_ref: "canonical".into(),
            title: "Canonical checkout has tracked dirty files".into(),
            state: "local".into(),
            severity: AuditSeverity::Blocker,
            code: "canonical_checkout_tracked_dirty".into(),
            message: "tracked dirty files".into(),
            suggestion: "clean checkout".into(),
        }],
        integration_gaps: Vec::new(),
        skill_readiness_summary: None,
    };
    let canonical = AutopilotCanonicalCheckout {
        safe_for_write: false,
        reason: Some("current branch is \"feature/test\", expected \"main\"".into()),
        ..clean_autopilot_canonical()
    };

    let plan = build_autopilot_plan_from_parts(AutopilotPlanInputs {
        workflow_path: Path::new("/tmp/WORKFLOW.md"),
        config: &config,
        adapter: &adapter,
        issues,
        doctor_report: doctor,
        canonical_checkout: canonical,
        runtime: clean_autopilot_runtime(),
        integration_gaps: Vec::new(),
    })
    .unwrap();

    assert_eq!(
        plan.readiness.status,
        "blocked_by_doctor_or_canonical_checkout"
    );
    assert!(plan
        .readiness
        .blockers
        .iter()
        .any(|blocker| blocker.contains("doctor_blockers=1")));
    assert!(plan
        .readiness
        .blockers
        .iter()
        .any(|blocker| blocker.contains("canonical_checkout=")));
}

#[test]
fn autopilot_plan_does_not_select_non_dispatchable_or_parked_states() {
    let mut dogfood = tracker_issue_with_ref("#330", "Dogfood session coordination", "Backlog");
    dogfood.labels.push("dogfood-session".into());
    let mut todo_dogfood = tracker_issue_with_ref("#335", "Dogfood: live lane run", "Todo");
    todo_dogfood.labels.push("dogfood-session".into());
    let issues = vec![
        dogfood,
        todo_dogfood,
        tracker_issue_with_ref("#331", "Done main lane", "Done"),
        tracker_issue_with_ref("#332", "Human parked", "Human Review"),
        tracker_issue_with_ref("#333", "Needs input", "Need Human Input"),
        tracker_issue_with_ref("#334", "Clarify me", "Need to Clarify"),
    ];

    let plan = test_autopilot_plan(issues);

    assert!(plan.lanes.iter().all(|lane| lane.selected_issue.is_none()));
    assert_eq!(
        plan.parked_queues
            .iter()
            .find(|queue| queue.name == "Dogfood / Coordination")
            .unwrap()
            .issues
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        vec!["#330", "#335"]
    );
}

fn tracker_issue_with_review_claim() -> TrackerIssue {
    let mut issue = tracker_issue("Agent Review");
    let claim = LaneClaim::active(
        &issue.identifier,
        LaneClaimLane::Review,
        LaneClaimActor::Gemini,
        LaneClaimSource::Manual,
        1_779_000_900_123,
    );
    issue.project_fields.insert(
        "Review Agent".into(),
        serde_json::Value::String(claim.render()),
    );
    issue
}

fn review_issue_with_ref(identifier: &str, title: &str) -> TrackerIssue {
    let mut issue = tracker_issue_with_ref(identifier, title, "Agent Review");
    let number = identifier.trim_start_matches('#');
    issue
        .linked_pull_requests
        .push(jade_symphony::model::LinkedPullRequest {
            number: number.parse().ok(),
            url: Some(format!(
                "https://github.com/Alive24/jade-symphony/pull/{number}"
            )),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            ..Default::default()
        });
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

fn active_runtime_state(identifier: &str) -> RuntimeState {
    let mut state = RuntimeState::active(
        RuntimeIssueState {
            id: "ISSUE_29".into(),
            identifier: identifier.into(),
        },
        "dry-run",
    );
    state.updated_at_ms = Some(1_000);
    state
}

fn runtime_reconcile_test_config(root: &Path) -> RuntimeConfig {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nartifacts:\n  root: {:?}\n  namespace: Alive24/jade-symphony\nobservability:\n  logs_root: {:?}\n---\nPrompt",
                root.display().to_string(),
                root.join("logs").display().to_string()
            ),
        )
        .unwrap();
    RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
}

fn main_tmux_session_record(issue_identifier: &str, status: SessionStatus) -> AgentSessionRecord {
    AgentSessionRecord {
        issue_id: Some("ISSUE_338".into()),
        issue_identifier: Some(issue_identifier.into()),
        issue_title: Some("Reconcile completed main tmux sessions after handoff".into()),
        lane: "main".into(),
        run_id: Some("20260520T0403Z-issue338-main-c91b".into()),
        thread: None,
        session_source: Some("loop".into()),
        claim_value: None,
        actor_role: Some("codex".into()),
        actor_label: Some("Codex manual main issue-338".into()),
        git_author: None,
        profile_id: None,
        instance_name: None,
        worktree: PathBuf::from("/tmp/issue-338"),
        branch: Some("feature/issue-338".into()),
        backend: "tmux".into(),
        session_name: "jade-main-338-attempt-1-reconcile".into(),
        pane_target: "jade-main-338-attempt-1-reconcile".into(),
        prompt_artifact_path: PathBuf::from("/tmp/prompt.md"),
        log_path: PathBuf::from("/tmp/session.log"),
        attach_command: "tmux attach-session -t jade-main-338-attempt-1-reconcile".into(),
        attempt: 1,
        status,
        started_at_ms: 1_000,
        updated_at_ms: 1_000,
    }
}

fn init_clean_git_workspace(path: &Path) {
    let output = ProcessCommand::new("git")
        .arg("init")
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
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
fn review_session_uses_gemini_command_when_no_tmux_override_exists() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: tmux\ntmux:\n  agent_command: codex\nreview_lane:\n  backend: gemini-cli\n  gemini_command: /opt/homebrew/bin/gemini\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    assert_eq!(
        tmux_agent_command_for_lane(&config, AgentSessionLaneArg::Main).unwrap(),
        "codex"
    );
    assert_eq!(
        tmux_agent_command_for_lane(&config, AgentSessionLaneArg::Review).unwrap(),
        "/opt/homebrew/bin/gemini"
    );
    assert_eq!(
        tmux_agent_command_for_lane(&config, AgentSessionLaneArg::Merge).unwrap(),
        "codex"
    );
}

#[test]
fn review_session_prefers_tmux_review_command_override() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: tmux\ntmux:\n  agent_command: codex\n  review_agent_command: custom-gemini --model pro\nreview_lane:\n  backend: gemini-cli\n  gemini_command: /opt/homebrew/bin/gemini\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    assert_eq!(
        tmux_agent_command_for_lane(&config, AgentSessionLaneArg::Review).unwrap(),
        "custom-gemini --model pro"
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
fn merge_session_defaults_to_codex_app_server_command() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\ncodex:\n  command: /opt/homebrew/bin/codex app-server\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    let spec = agent_session_backend_spec(&config, AgentSessionLaneArg::Merge).unwrap();

    assert_eq!(spec.backend, "codex");
    assert_eq!(spec.command, "/opt/homebrew/bin/codex app-server");
}

#[test]
fn merge_session_keeps_tmux_as_explicit_fallback() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmerge_lane:\n  agent_backend: tmux\ntmux:\n  agent_command: codex\n  merge_agent_command: codex --profile merge\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    let spec = agent_session_backend_spec(&config, AgentSessionLaneArg::Merge).unwrap();

    assert_eq!(spec.backend, "tmux");
    assert_eq!(spec.command, "codex --profile merge");
}

#[test]
fn clean_merge_tick_does_not_require_merge_agent_backend() {
    let temp = tempfile::tempdir().unwrap();
    let workflow_path = temp.path().join("WORKFLOW.md");
    std::fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: memory\nmerge_lane:\n  agent_backend: definitely-not-a-session-backend\n---\nPrompt",
        )
        .unwrap();

    let outcome = merge_once_tick(workflow_path, false, false).unwrap();

    assert_eq!(outcome, MergeOnceOutcome::NoMergingIssue);
}

#[test]
fn successful_merge_agent_repair_records_merging_retry_rationale() {
    let config = test_config();
    let issue = tracker_issue_with_ref("#390", "Route DIRTY repair", "Merging");
    let runner = MergeRecoveryRunner::new();

    let outcome = finish_merge_agent_repaired_branch(
        &config,
        &issue,
        "merge_agent",
        "src/main.rs conflicted",
        "resolved conflict by preserving approved behavior",
        "approved implementation intent preserved",
        vec![
            "git diff --name-only --diff-filter=U".into(),
            "git diff --check".into(),
            "git status --porcelain".into(),
        ],
        "https://github.com/Alive24/jade-symphony/pull/390",
        "feature/issue-390",
        &runner,
        Path::new("."),
        CommandOutput {
            status: 0,
            stdout: "MERGE_AGENT_DECISION: repaired".into(),
            stderr: String::new(),
        },
        "codex".into(),
        Some("session-390".into()),
    )
    .unwrap();

    assert!(outcome.repaired);
    assert_eq!(outcome.evidence.method, "merge_agent");
    assert!(outcome
        .evidence
        .next_state_rationale
        .contains("stays in `Merging`"));
    assert!(runner
        .calls
        .borrow()
        .iter()
        .any(|call| call == "git push origin feature/issue-390"));
}

#[test]
fn merge_agent_semantic_uncertainty_marker_requires_human_input() {
    let text = "\
RESOLUTION_SUMMARY: conflict needs product choice
SEMANTIC_SAFETY: cannot prove reviewed intent
MERGE_AGENT_DECISION: needs_human_input";

    assert!(merge_agent_requests_human_input(text));
    assert!(!merge_agent_reports_repaired(text));
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
fn main_app_server_smoke_gate_is_ready_for_codex_app_server() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: codex\ncodex:\n  command: /opt/homebrew/bin/codex app-server\n  approval_policy: never\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    let gate = main_app_server_smoke_gate(&config);

    assert_eq!(gate.backend, "codex");
    assert_eq!(gate.backend_source, "codex-app-server");
    assert_eq!(gate.command, "/opt/homebrew/bin/codex app-server");
    assert_eq!(gate.approval_policy, "never");
    assert!(gate.app_server_live_smoke_ready);
}

#[test]
fn main_app_server_smoke_gate_rejects_non_app_server_codex() {
    let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: codex\ncodex:\n  command: codex exec\n---\nPrompt",
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

    let gate = main_app_server_smoke_gate(&config);

    assert_eq!(gate.backend, "codex");
    assert_eq!(gate.backend_source, "codex-subprocess");
    assert!(!gate.app_server_live_smoke_ready);
    assert!(gate
        .app_server_live_smoke_reason
        .contains("does not select the app-server"));
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

#[test]
fn automatic_review_prompt_forbids_project_mutations() {
    let workflow = WorkflowDefinition::parse(
        "/tmp/WORKFLOW.md",
        "---\ntracker:\n  kind: memory\n---\nReview {{ issue.identifier }}",
    )
    .unwrap();
    let prompt = render_automatic_review_prompt(
        &workflow,
        &review_issue_with_ref("#282", "Headless review"),
    )
    .unwrap();

    assert!(prompt.contains("Review #282"));
    assert!(prompt.contains("Automatic Headless Review Boundary"));
    assert!(prompt.contains("Do not run mutating Jade Symphony or GitHub commands"));
    assert!(prompt.contains("`review claim`, `review pass`"));
    assert!(prompt.contains("`gh issue edit`, `gh issue comment`"));
    assert!(prompt.contains("Return review evidence in stdout only"));
    assert!(prompt.contains("Review Result: PASS"));
    assert!(prompt.contains("Do not use those bracketed finding tags for positive"));
    assert!(prompt.contains("Leave routing and evidence"));
}

#[test]
fn manual_review_pass_workpad_records_doctor_evidence_marker() {
    let issue = tracker_issue_with_review_claim();
    let claim = project_text_field(&issue, "Review Agent").unwrap();
    let terminal = format!(
        "{} result=passed",
        LaneClaim::parse(&claim)
            .unwrap()
            .with_state(LaneClaimState::Done)
            .render()
    );
    let workpad = render_manual_review_workpad(
        &issue,
        "passed",
        "human_review",
        "Gemini: pass",
        true,
        &claim,
        &terminal,
    );

    assert!(workpad.contains("Reviewer backend: manual-operator"));
    assert!(workpad.contains("Review pass evidence: `recorded`"));
    assert!(workpad.contains("main implementation agent must not"));
    assert!(workpad.contains("Terminal Review Agent claim"));
}

#[test]
fn manual_review_reject_workpad_does_not_record_pass_marker() {
    let issue = tracker_issue_with_review_claim();
    let claim = project_text_field(&issue, "Review Agent").unwrap();
    let terminal = format!(
        "{} result=inconclusive",
        LaneClaim::parse(&claim)
            .unwrap()
            .with_state(LaneClaimState::Failed)
            .render()
    );
    let workpad = render_manual_review_workpad(
        &issue,
        "not passed",
        "agent_review",
        "Gemini: inconclusive",
        false,
        &claim,
        &terminal,
    );

    assert!(!workpad.contains("Review pass evidence: `recorded`"));
    assert!(workpad.contains("must not move to Human Review"));
}

#[test]
fn manual_review_claim_validation_requires_exact_evidence_claim() {
    let issue = tracker_issue_with_review_claim();
    let claim = project_text_field(&issue, "Review Agent").unwrap();

    assert!(validate_active_manual_review_claim(&issue, &format!("claim: {claim}")).is_ok());
    let error = validate_active_manual_review_claim(&issue, "claim: Manual Gemini A")
        .unwrap_err()
        .to_string();
    assert!(error.contains("exact current Review Agent claim"));
}

#[test]
fn manual_review_pass_allows_terminal_passed_claim_repair() {
    let mut issue = tracker_issue_with_review_claim();
    let claim = project_text_field(&issue, "Review Agent").unwrap();
    let terminal = terminal_review_claim_value(
        &LaneClaim::parse(&claim).unwrap(),
        LaneClaimState::Done,
        "passed",
    );
    issue.project_fields.insert(
        "Review Agent".into(),
        serde_json::Value::String(terminal.clone()),
    );

    let (current, parsed) =
        validate_manual_review_pass_claim(&issue, &format!("claim: {terminal}")).unwrap();

    assert_eq!(current, terminal);
    assert_eq!(parsed.state, LaneClaimState::Done);
}

#[test]
fn manual_review_reject_still_requires_active_claim() {
    let mut issue = tracker_issue_with_review_claim();
    let claim = project_text_field(&issue, "Review Agent").unwrap();
    let terminal = terminal_review_claim_value(
        &LaneClaim::parse(&claim).unwrap(),
        LaneClaimState::Done,
        "passed",
    );
    issue.project_fields.insert(
        "Review Agent".into(),
        serde_json::Value::String(terminal.clone()),
    );

    let error = validate_active_manual_review_claim(&issue, &format!("claim: {terminal}"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("must be active before routing"));
}

#[test]
fn terminal_review_claim_records_result_without_losing_structured_claim() {
    let issue = tracker_issue_with_review_claim();
    let claim = LaneClaim::parse(&project_text_field(&issue, "Review Agent").unwrap()).unwrap();

    let value = terminal_review_claim_value(&claim, LaneClaimState::Done, "passed");

    assert!(value.contains("state=done"));
    assert!(value.contains("result=passed"));
    assert_eq!(
        LaneClaim::parse(&value)
            .unwrap()
            .with_state(LaneClaimState::Active),
        claim
    );
}

#[test]
fn review_worker_selection_respects_concurrency_limit() {
    let selected = select_review_worker_issues(
        &[
            review_issue_with_ref("#67", "First review"),
            review_issue_with_ref("#68", "Second review"),
            review_issue_with_ref("#69", "Third review"),
        ],
        "Agent Review",
        "fake-reviewer",
        2,
    );

    assert_eq!(
        selected
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        vec!["#67", "#68"]
    );
}

#[test]
fn review_worker_selection_skips_existing_worker_marker() {
    let mut queued = review_issue_with_ref("#67", "Queued review");
    queued.project_fields.insert(
        "Review Worker".into(),
        serde_json::Value::String("queued review:#67:fake-reviewer".into()),
    );
    let ready = review_issue_with_ref("#68", "Ready review");

    let selected =
        select_review_worker_issues(&[queued, ready], "Agent Review", "fake-reviewer", 2);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].identifier, "#68");
}

#[test]
fn review_worker_selection_skips_review_agent_field_claim() {
    let mut queued = review_issue_with_ref("#67", "Queued review");
    let claim = review_claim_for_issue(&queued, "review:#67:fake-reviewer");
    queued.project_fields.insert(
        "Review Agent".into(),
        serde_json::Value::String(claim.render()),
    );
    let ready = review_issue_with_ref("#68", "Ready review");

    let selected =
        select_review_worker_issues(&[queued, ready], "Agent Review", "fake-reviewer", 2);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].identifier, "#68");
}

#[test]
fn review_claim_for_issue_replaces_terminal_review_claim() {
    let mut issue = review_issue_with_ref("#67", "Retry review");
    let terminal_claim = LaneClaim::active(
        "#67",
        LaneClaimLane::Review,
        LaneClaimActor::Gemini,
        LaneClaimSource::Loop,
        42,
    )
    .with_worker("review:#67:gemini-cli")
    .with_state(LaneClaimState::Failed);
    issue.project_fields.insert(
        "Review Agent".into(),
        serde_json::Value::String(format!("{} result=inconclusive", terminal_claim.render())),
    );

    let claim = review_claim_for_issue(&issue, "review:#67:gemini-cli");

    assert_eq!(claim.state, LaneClaimState::Active);
    assert_ne!(claim.run, terminal_claim.run);
}

#[test]
fn review_loop_terminal_claim_records_pass_result() {
    let claim = LaneClaim::active(
        "#67",
        LaneClaimLane::Review,
        LaneClaimActor::Gemini,
        LaneClaimSource::Loop,
        42,
    )
    .with_worker("review:#67:gemini-cli");
    let decision = ReviewGateDecision {
        outcome: ReviewOutcome::PassedToHumanReview,
        target_state: Some("human_review"),
        message: "passed".into(),
    };
    let job = ReviewJob {
        id: "job".into(),
        issue_ref: "#67".into(),
        backend: "gemini-cli".into(),
        state: ReviewJobState::Completed,
        artifact_path: None,
        ledger_path: None,
        report: None,
        error: None,
    };

    let value = terminal_review_loop_claim_value(Some(&claim), &job, &decision).unwrap();

    assert!(value.contains("state=done"));
    assert!(value.contains("result=passed"));
    assert_eq!(
        LaneClaim::parse(&value).unwrap(),
        claim.with_state(LaneClaimState::Done)
    );
}

#[test]
fn review_pass_checklist_update_checks_non_uat_sections_only() {
    let body = [
        "## Expected Outcome",
        "",
        "- [ ] Outcome done",
        "",
        "## Verification",
        "",
        "### Completion Criteria",
        "",
        "- [ ] Criteria done",
        "",
        "### Functional Verification",
        "",
        "- [ ] `cargo test`",
        "",
        "### UAT",
        "",
        "- [ ] Human checks this",
        "",
        "### Context Verification",
        "",
        "- [ ] Context done",
        "",
        "```md",
        "- [ ] do not touch fenced examples",
        "```",
    ]
    .join("\n");

    let updated = check_review_verified_issue_body_checkboxes(&body);

    assert!(updated.contains("- [x] Outcome done"));
    assert!(updated.contains("- [x] Criteria done"));
    assert!(updated.contains("- [x] `cargo test`"));
    assert!(updated.contains("- [ ] Human checks this"));
    assert!(updated.contains("- [x] Context done"));
    assert!(updated.contains("- [ ] do not touch fenced examples"));
}

#[test]
fn review_pass_checklist_update_removes_appended_workpad_before_editing_body() {
    let description =
        "## Expected Outcome\n\n- [ ] Done\n\n<!-- jade-symphony-workpad -->\n## Agent Review";

    let body = canonical_issue_body_without_workpad(description);
    let updated = check_review_verified_issue_body_checkboxes(&body);

    assert_eq!(updated, "## Expected Outcome\n\n- [x] Done");
    assert!(!updated.contains("jade-symphony-workpad"));
}

#[test]
fn review_pass_updates_issue_body_checkboxes_before_human_review_transition() {
    let config = test_config();
    let adapter = RecordingAdapter::default();
    let mut issue = review_issue_with_ref("#67", "Checklist review");
    issue.description = Some(
        [
            "## Expected Outcome",
            "",
            "- [ ] Outcome done",
            "",
            "## Verification",
            "",
            "### Completion Criteria",
            "",
            "- [ ] Criteria done",
            "",
            "### Functional Verification",
            "",
            "- [ ] `cargo test`",
            "",
            "### UAT",
            "",
            "- [ ] Human checks this",
            "",
            "### Context Verification",
            "",
            "- [ ] Context done",
        ]
        .join("\n"),
    );
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue.clone());
    let job = ReviewJob {
        id: "job-67".into(),
        issue_ref: "#67".into(),
        backend: "gemini-cli".into(),
        state: ReviewJobState::Completed,
        artifact_path: None,
        ledger_path: None,
        report: Some(jade_symphony::review::AgentReviewReport {
            summary: Some("Review Result: PASS".into()),
            ..Default::default()
        }),
        error: None,
    };

    apply_review_result(&config, &adapter, "#67", &issue, &job, None, None).unwrap();

    let updated = adapter
        .issues
        .borrow()
        .get("#67")
        .and_then(|issue| issue.description.clone())
        .unwrap();
    assert!(updated.contains("- [x] Outcome done"));
    assert!(updated.contains("- [x] Criteria done"));
    assert!(updated.contains("- [x] `cargo test`"));
    assert!(updated.contains("- [ ] Human checks this"));
    assert!(updated.contains("- [x] Context done"));
    assert_eq!(
        adapter.operations(),
        vec![
            "update_issue_content:#67",
            "comment:#67",
            "set_state:#67:human_review"
        ]
    );
}

#[test]
fn review_workspace_uses_issue_handoff_workspace() {
    let config = test_config();
    let issue = review_issue_with_ref("#67", "Add parallel review worker pool");

    let workspace = review_workspace_for_issue(&config, &issue);

    assert!(workspace.ends_with("issue-67-add-parallel-review-worker-pool"));
}

#[test]
fn pool_worker_selection_respects_lane_priority_and_claim_owner() {
    let config = test_config();
    let worker = "Jade Symphony Main";
    let mut first = tracker_issue_with_ref("#1", "First", "Todo");
    first.priority = Some(20);
    let mut second = tracker_issue_with_ref("#2", "Second", "Rework");
    second.priority = Some(10);
    let mut owned_by_other = tracker_issue_with_ref("#3", "Other owned", "Todo");
    owned_by_other.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String("Another Main".into()),
    );
    let mut owned_by_self = tracker_issue_with_ref("#4", "Self owned", "In Progress");
    owned_by_self.priority = Some(5);
    owned_by_self.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String(worker.into()),
    );
    let merging = tracker_issue_with_ref("#5", "Merging", "Merging");

    let selected = select_pool_worker_issues(
        &[first, second, owned_by_other, owned_by_self, merging],
        WorkerLane::Main,
        worker,
        2,
        &config,
    );

    assert_eq!(
        selected
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        vec!["#4", "#2"]
    );
}

#[test]
fn pool_worker_selection_returns_empty_when_no_slots_remain() {
    let config = test_config();
    let issue = tracker_issue_with_ref("#1", "Ready", "Todo");

    let selected = select_pool_worker_issues(&[issue], WorkerLane::Main, "worker", 0, &config);

    assert!(selected.is_empty());
}

#[test]
fn main_run_loop_selection_prioritizes_recovery_and_fills_remaining_slots() {
    let config = test_config();
    let worker = "Jade Symphony Main";
    let recovery_issue = tracker_issue_with_ref("#362", "Recover me", "In Progress");
    let mut next_todo = tracker_issue_with_ref("#363", "Start next", "Todo");
    next_todo.priority = Some(1);
    let recovery = RuntimeRecoveryCandidate {
        state: active_runtime_state("#362"),
        issue: recovery_issue,
        reason: "retry_due attempt=2 error=HTTP 429".into(),
    };

    let selected = select_main_run_loop_issues(&[recovery], &[next_todo], 2, worker, &config);

    assert_eq!(
        selected
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        vec!["#362", "#363"]
    );
}

#[cfg(unix)]
#[test]
fn main_run_loop_write_dispatch_starts_selected_candidates_concurrently() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let codex = bin_dir.join("codex");
    let start_log = temp.path().join("codex-starts.log");
    std::fs::write(
            &codex,
            format!(
                r#"#!/bin/sh
set -eu
start_log={}
count=0
while IFS= read -r line; do
  count=$((count + 1))
  case "$count" in
    1)
      printf '%s\n' '{{"id":1,"result":{{}}}}'
      ;;
    2)
      ;;
    3)
      printf '{{"id":2,"result":{{"thread":{{"id":"thread-%s"}}}}}}\n' "$$"
      ;;
    4)
      printf '{{"id":3,"result":{{"turn":{{"id":"turn-%s"}}}}}}\n' "$$"
      printf '%s\n' "$$" >> "$start_log"
      remaining=40
      while [ "$remaining" -gt 0 ]; do
        starts="$(wc -l < "$start_log" 2>/dev/null || echo 0)"
        [ "$starts" -ge 2 ] && break
        remaining=$((remaining - 1))
        sleep 0.05
      done
      starts="$(wc -l < "$start_log" 2>/dev/null || echo 0)"
      if [ "$starts" -lt 2 ]; then
        printf '%s\n' '{{"method":"turn/failed","params":{{"error":{{"message":"second worker did not start before timeout"}}}}}}'
        exit 0
      fi
      printf '%s\n' '{{"method":"thread/tokenUsage/updated","params":{{"inputTokens":1,"outputTokens":1,"totalTokens":2}}}}'
      printf '%s\n' '{{"method":"turn/completed","params":{{"turn":{{"status":"completed"}}}}}}'
      exit 0
      ;;
  esac
done
"#,
                shell_quote_display(&start_log.display().to_string())
            ),
        )
        .unwrap();
    let mut permissions = std::fs::metadata(&codex).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&codex, permissions).unwrap();

    let workflow_path = temp.path().join("WORKFLOW.md");
    let workflow = WorkflowDefinition::parse(
            &workflow_path,
            &format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {}\nartifacts:\n  root: {}\nobservability:\n  logs_root: {}\nmain_lane:\n  backend: codex\ncodex:\n  command: {} app-server\n  read_timeout_ms: 1000\n  turn_timeout_ms: 5000\n---\nPrompt for {{{{ issue.identifier }}}}",
                temp.path().join("worktrees").display(),
                temp.path().join("artifacts").display(),
                temp.path().join("logs").display(),
                shell_quote_display(&codex.display().to_string())
            ),
        )
        .unwrap();
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path).unwrap();
    let mut first = tracker_issue_with_ref("#362", "Recover first", "In Progress");
    first.id = "ISSUE_362".into();
    first.description = Some(forge_contract());
    let mut second = tracker_issue_with_ref("#363", "Start second", "In Progress");
    second.id = "ISSUE_363".into();
    second.description = Some(forge_contract());
    let options = RunLoopOptions {
        workflow_path,
        max_iterations: Some(1),
        once: false,
        write: true,
        recover: true,
        max_concurrent: Some(2),
        display: DisplayMode::Plain,
    };

    run_loop_dispatch_write_candidates(
        &workflow,
        &config,
        vec![first, second],
        &options,
        true,
        1,
        2,
    )
    .unwrap();

    let starts = std::fs::read_to_string(start_log).unwrap();
    assert_eq!(starts.lines().count(), 2);
}

#[test]
fn merge_pool_selection_only_accepts_merging_lane() {
    let config = test_config();
    let mut claimed = tracker_issue_with_ref("#6", "Claimed merge", "Merging");
    claimed.project_fields.insert(
        "Merging Agent".into(),
        serde_json::Value::String("other merger".into()),
    );
    let mut unclaimed = tracker_issue_with_ref("#7", "Ready merge", "Merging");
    unclaimed.priority = Some(1);
    let todo = tracker_issue_with_ref("#8", "Main work", "Todo");

    let selected = select_pool_worker_issues(
        &[claimed, unclaimed, todo],
        WorkerLane::Merging,
        "this merger",
        4,
        &config,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].identifier, "#7");
}

#[test]
fn merge_pool_selection_reuses_structured_active_claim_for_same_worker() {
    let config = test_config();
    let worker = "Jade Symphony Agent";
    let claim = LaneClaim::active(
        "#6",
        LaneClaimLane::Merge,
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        1_779_000_000_000,
    )
    .with_worker(worker);
    let mut claimed_by_self = tracker_issue_with_ref("#6", "Claimed merge", "Merging");
    claimed_by_self.project_fields.insert(
        "Merging Agent".into(),
        serde_json::Value::String(claim.render()),
    );

    let selected =
        select_pool_worker_issues(&[claimed_by_self], WorkerLane::Merging, worker, 1, &config);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].identifier, "#6");
}

#[test]
fn merge_recover_selection_prioritizes_interrupted_loop_claims() {
    let config = test_config();
    let worker = "Jade Symphony Agent";
    let interrupted_claim = LaneClaim::active(
        "#6",
        LaneClaimLane::Merge,
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        1_779_000_000_000,
    )
    .with_worker("previous merger");
    let mut interrupted = tracker_issue_with_ref("#6", "Interrupted merge", "Merging");
    interrupted.priority = Some(20);
    interrupted.project_fields.insert(
        "Merging Agent".into(),
        serde_json::Value::String(interrupted_claim.render()),
    );
    let mut unclaimed = tracker_issue_with_ref("#7", "Ready merge", "Merging");
    unclaimed.priority = Some(1);

    let selected = select_merge_worker_issues(&[unclaimed, interrupted], worker, 1, &config, true);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].issue.identifier, "#6");
    assert!(selected[0]
        .recovery_reason
        .as_deref()
        .unwrap()
        .contains("previous_worker=previous merger"));
}

#[test]
fn merge_recover_selection_does_not_adopt_manual_claims() {
    let config = test_config();
    let worker = "Jade Symphony Agent";
    let manual_claim = LaneClaim::active(
        "#6",
        LaneClaimLane::Merge,
        LaneClaimActor::Codex,
        LaneClaimSource::Manual,
        1_779_000_000_000,
    )
    .with_worker("manual merger");
    let mut manual = tracker_issue_with_ref("#6", "Manual merge", "Merging");
    manual.project_fields.insert(
        "Merging Agent".into(),
        serde_json::Value::String(manual_claim.render()),
    );
    let unclaimed = tracker_issue_with_ref("#7", "Ready merge", "Merging");

    let selected = select_merge_worker_issues(&[manual, unclaimed], worker, 2, &config, true);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].issue.identifier, "#7");
    assert!(selected[0].recovery_reason.is_none());
}

#[test]
fn pool_claim_eligibility_reports_existing_owner() {
    let config = test_config();
    let mut issue = tracker_issue("Todo");
    issue.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String("someone else".into()),
    );

    assert_eq!(
        pool_claim_eligibility(&issue, WorkerLane::Main, "this worker", &config),
        PoolClaimEligibility::ClaimedByOther {
            owner: "someone else".into()
        }
    );
}

#[test]
fn run_loop_claim_action_uses_tracker_claim_decision() {
    let config = test_config();

    assert_eq!(
        run_loop_claim_action(&tracker_issue("Todo"), &config),
        RunLoopClaimAction::Claim
    );
    assert_eq!(
        run_loop_claim_action(&tracker_issue("Rework"), &config),
        RunLoopClaimAction::Claim
    );
    assert_eq!(
        run_loop_claim_action(&tracker_issue("In Progress"), &config),
        RunLoopClaimAction::Resume
    );
    assert_eq!(
        run_loop_claim_action(&tracker_issue("Agent Review"), &config),
        RunLoopClaimAction::StopAndReplan {
            current_state: "Agent Review".into()
        }
    );
}

#[test]
fn live_gate_blocks_missing_assignee_without_override() {
    let config = live_github_config(false);
    let issue = tracker_issue("Todo");

    assert_eq!(
        live_missing_assignee_gate_blocker(&config, &issue).as_deref(),
        Some("live GitHub issue assignee")
    );
}

#[test]
fn issue_contract_assignees_parse_setup_field() {
    assert_eq!(
        issue_contract_assignees("- Assignee: @Alive24\n- UAT Required: Yes"),
        vec!["Alive24".to_string()]
    );
    assert_eq!(
        issue_contract_assignees("- Assignees: Alive24, codex\n"),
        vec!["Alive24".to_string(), "codex".to_string()]
    );
}

#[test]
fn fixture_mode_does_not_require_live_assignee() {
    let config = fixture_github_config();
    let issue = tracker_issue("Todo");

    assert_eq!(live_missing_assignee_gate_blocker(&config, &issue), None);
    assert_eq!(
        run_loop_assignee_ownership_decision(&issue, &config, None, None),
        AssigneeOwnershipDecision::Allowed
    );
}

#[test]
fn assignee_ownership_allows_matching_active_login() {
    let config = live_github_config(false);
    let mut issue = tracker_issue("Todo");
    issue.assignees = vec!["CodexUser".into()];

    assert_eq!(
        run_loop_assignee_ownership_decision(&issue, &config, Some("codexuser"), None),
        AssigneeOwnershipDecision::Allowed
    );
}

#[test]
fn assignee_ownership_blocks_mismatched_active_login() {
    let config = live_github_config(false);
    let mut issue = tracker_issue("Todo");
    issue.assignees = vec!["owner-a".into()];

    let decision = run_loop_assignee_ownership_decision(&issue, &config, Some("owner-b"), None);

    assert!(matches!(decision, AssigneeOwnershipDecision::Block { .. }));
}

#[test]
fn assignee_ownership_allows_matching_profile_login() {
    let config = live_github_config(false);
    let mut issue = tracker_issue("Todo");
    issue.assignees = vec!["profile-owner".into()];

    assert_eq!(
        run_loop_assignee_ownership_decision(
            &issue,
            &config,
            Some("different-gh-user"),
            Some("profile-owner"),
        ),
        AssigneeOwnershipDecision::Allowed
    );
}

#[test]
fn assignee_ownership_blocks_missing_active_identity() {
    let config = live_github_config(false);
    let mut issue = tracker_issue("Todo");
    issue.assignees = vec!["owner-a".into()];

    let decision = run_loop_assignee_ownership_decision(&issue, &config, None, None);

    assert_eq!(
        decision,
        AssigneeOwnershipDecision::Block {
            reason: "active GitHub identity unavailable for assignee ownership check".into(),
        }
    );
}

#[test]
fn run_loop_runtime_ownership_workpad_records_matching_marker() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let ownership = run_loop_runtime_ownership(&issue, &config, &handoff).unwrap();
    let claim = test_claim(&issue);

    let workpad = run_loop_ownership_workpad(&issue, &ownership, "Resumed", &claim);

    assert!(workpad.contains("jade-symphony-runtime-ownership"));
    assert_eq!(
        runtime_ownership_decision(Some(&workpad), &ownership),
        RuntimeOwnershipDecision::Matches
    );
}

#[test]
fn run_loop_runtime_ownership_detects_different_active_branch() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let expected = run_loop_runtime_ownership(&issue, &config, &handoff).unwrap();
    let mut existing = expected.clone();
    existing.branch_name = "feature/issue-100-other-work".into();
    let workpad = render_runtime_ownership_marker(&existing);

    assert!(matches!(
        runtime_ownership_decision(Some(&workpad), &expected),
        RuntimeOwnershipDecision::Mismatched { .. }
    ));
}

#[test]
fn resume_preflight_continues_active_in_progress_state() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let state = active_runtime_state("#29");

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert_eq!(action, ResumePreflightAction::Continue);
}

#[test]
fn resume_preflight_archives_non_active_state_with_absent_worktree() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("Need to Clarify")]);
    let state = active_runtime_state("#29");

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert_eq!(
        action,
        ResumePreflightAction::ArchiveStale {
            issue_identifier: "#29".into(),
            tracker_state: "Need to Clarify".into(),
            archive_reason: "tracker_state_non_active".into(),
        }
    );
}

#[test]
fn resume_preflight_archives_terminal_state_with_clean_worktree() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("Done")]);
    let temp = tempfile::tempdir().unwrap();
    init_clean_git_workspace(temp.path());
    let mut state = active_runtime_state("#29");
    state.workspace_path = Some(temp.path().to_path_buf());

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert_eq!(
        action,
        ResumePreflightAction::ArchiveStale {
            issue_identifier: "#29".into(),
            tracker_state: "Done".into(),
            archive_reason: "tracker_state_terminal".into(),
        }
    );
}

#[test]
fn resume_preflight_blocks_non_active_state_with_dirty_worktree() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("Need to Clarify")]);
    let temp = tempfile::tempdir().unwrap();
    init_clean_git_workspace(temp.path());
    std::fs::write(temp.path().join("scratch.txt"), "dirty work").unwrap();
    let mut state = active_runtime_state("#29");
    state.workspace_path = Some(temp.path().to_path_buf());

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert!(
        matches!(action, ResumePreflightAction::Block { reason } if reason.contains("workspace is dirty"))
    );
}

#[test]
fn resume_preflight_archive_allows_unrelated_todo_selection() {
    let config = main_loop_test_config();
    let stale = tracker_issue_with_ref("#29", "Needs clarification", "Need to Clarify");
    let mut todo = tracker_issue_with_ref("#30", "Ready next work", "Todo");
    todo.description = Some(forge_contract());
    let tracker = MemoryTracker::new(vec![stale, todo.clone()]);
    let state = active_runtime_state("#29");

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();
    let plan = Orchestrator::new(config).plan_dispatch(tracker.list_dispatchable_issues().unwrap());

    assert!(matches!(action, ResumePreflightAction::ArchiveStale { .. }));
    assert_eq!(
        plan.selected.first().map(|issue| issue.identifier.as_str()),
        Some("#30")
    );
}

#[test]
fn resume_preflight_many_counts_active_main_worker_slots() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    let tracker = MemoryTracker::new(vec![
        tracker_issue_with_ref("#29", "Runtime one", "In Progress"),
        tracker_issue_with_ref("#30", "Runtime two", "In Progress"),
    ]);
    let states = vec![active_runtime_state("#29"), active_runtime_state("#30")];

    let summary = run_loop_resume_preflight_many(&tracker, &config, &states, 2_000, false).unwrap();

    assert_eq!(summary.active_main_workers, 2);
    assert_eq!(summary.retained_states.len(), 2);
    assert_eq!(summary.blocked, None);
}

#[test]
fn resume_preflight_many_archives_only_stale_slot() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![
        tracker_issue_with_ref("#29", "Handed off", "Agent Review"),
        tracker_issue_with_ref("#30", "Still active", "In Progress"),
    ]);
    let states = vec![active_runtime_state("#29"), active_runtime_state("#30")];

    let summary = run_loop_resume_preflight_many(&tracker, &config, &states, 2_000, false).unwrap();

    assert_eq!(summary.active_main_workers, 1);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(
        summary.retained_states[0]
            .active_issue
            .as_ref()
            .map(|issue| issue.identifier.as_str()),
        Some("#30")
    );
}

#[test]
fn resume_preflight_many_marks_stalled_state_recoverable_when_requested() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    state.updated_at_ms = Some(1_000);

    let summary = run_loop_resume_preflight_many(
        &tracker,
        &config,
        &[state],
        config.codex.stall_timeout_ms + 2_000,
        true,
    )
    .unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 1);
}

#[test]
fn resume_preflight_many_prefers_completed_app_server_session_over_stale_runtime_clock() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    state.backend = "codex".into();
    state.last_event = Some("Resumed".into());
    state.backend_session_id = None;
    state.updated_at_ms = Some(1_000);

    let mut record = main_tmux_session_record("#29", SessionStatus::Completed);
    record.backend = "codex".into();
    record.session_source = Some("codex-app-server".into());
    record.session_name = "thread-29-turn-1".into();
    record.pane_target = String::new();
    record.log_path = temp.path().join("logs/app-server/29.events.json");
    record.updated_at_ms = 2_000;
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(
        &tracker,
        &config,
        &[state],
        config.codex.stall_timeout_ms + 2_000,
        true,
    )
    .unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("status=completed"));
    assert_eq!(
        summary.recoverable_states[0].state.last_event.as_deref(),
        Some("SessionTerminal")
    );
    assert_eq!(
        summary.recoverable_states[0]
            .state
            .backend_session_id
            .as_deref(),
        Some("thread-29-turn-1")
    );
}

#[test]
fn resumed_pending_session_state_preserves_backend_session_for_reconciliation() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let mut existing = active_runtime_state(&issue.identifier);
    existing.backend = "codex".into();
    existing.last_event = Some("SessionTerminal".into());
    existing.backend_session_id = Some("thread-29-turn-1".into());
    existing.backend_log_path = Some(PathBuf::from("/tmp/29.events.json"));
    existing.workspace_path = Some(PathBuf::from("/tmp/issue-29"));

    let state =
        run_loop_runtime_state_for_issue(Some(&existing), &issue, &config, "Resumed", &claim);

    assert_eq!(state.last_event.as_deref(), Some("SessionTerminal"));
    assert_eq!(
        state.backend_session_id.as_deref(),
        Some("thread-29-turn-1")
    );
    assert_eq!(state.attempt_count, existing.attempt_count);
}

#[test]
fn resume_preflight_many_marks_missing_session_registry_recoverable() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    state.backend = "tmux".into();
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("jade-main-missing".into());

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[state], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.recoverable_states.len(), 1);
}

#[cfg(unix)]
#[test]
fn resume_preflight_many_counts_running_tmux_session_in_recover_mode() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_capture_script(
        temp.path(),
        "Codex\n◦ Running cargo run -- autopilot plan\n› Improve documentation in @filename",
    );
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    state.backend = "tmux".into();
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("jade-main-29-attempt-1".into());
    state.updated_at_ms = Some(1_000);

    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    record.updated_at_ms = 1_000;
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(
        &tracker,
        &config,
        &[state],
        config.codex.stall_timeout_ms + 2_000,
        true,
    )
    .unwrap();

    assert_eq!(summary.active_main_workers, 1);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 0);
}

#[cfg(unix)]
#[test]
fn resume_preflight_many_counts_registry_only_running_tmux_session_in_recover_mode() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_capture_script(
        temp.path(),
        "Codex\n◦ Running cargo test\n› Improve documentation in @filename",
    );
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    record.updated_at_ms = 1_000;
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(
        &tracker,
        &config,
        &[],
        config.codex.stall_timeout_ms + 2_000,
        true,
    )
    .unwrap();

    assert_eq!(summary.active_main_workers, 1);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 0);
    assert_eq!(
        runtime_state_issue_identifier(&summary.retained_states[0]),
        Some("#29")
    );
}

#[test]
fn resume_preflight_many_recovers_registry_only_failed_app_server_session() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut record = main_tmux_session_record("#29", SessionStatus::Failed);
    record.backend = "codex".into();
    record.session_source = Some("codex-app-server".into());
    record.session_name = "thread-29-turn-1".into();
    record.pane_target = String::new();
    record.attach_command =
        "not a tmux session; inspect app-server artifacts for recovery evidence".into();
    record.log_path = temp.path().join("logs/app-server/29.events.json");
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("status=failed"));
    assert_eq!(
        runtime_state_issue_identifier(&summary.retained_states[0]),
        Some("#29")
    );
}

#[cfg(unix)]
#[test]
fn resume_preflight_registry_active_session_does_not_require_live_issue_read() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_capture_script(
        temp.path(),
        "Codex\n◦ Running cargo test\n› Improve documentation in @filename",
    );
    let tracker = RecordingAdapter {
        fail_get_issue: true,
        ..Default::default()
    };
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 1);
    assert_eq!(summary.recoverable_states.len(), 0);
    assert_eq!(summary.retained_states.len(), 1);
}

#[cfg(unix)]
#[test]
fn resume_preflight_registry_active_session_skips_non_in_progress_tracker_state() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_capture_script(
        temp.path(),
        "Codex\n◦ Running cargo test\n› Improve documentation in @filename",
    );
    let tracker = MemoryTracker::new(vec![tracker_issue("Agent Review")]);
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.recoverable_states.len(), 0);
    assert_eq!(summary.retained_states.len(), 0);
}

#[cfg(unix)]
#[test]
fn resume_preflight_prefers_running_sibling_session_over_interrupted_runtime_session() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_split_session_script(temp.path());
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut attempt_two = main_tmux_session_record("#29", SessionStatus::Running);
    attempt_two.session_name = "jade-main-29-attempt-2".into();
    attempt_two.pane_target = "jade-main-29-attempt-2".into();
    attempt_two.log_path = temp.path().join("attempt-2.log");
    attempt_two.attempt = 2;
    let mut attempt_three = main_tmux_session_record("#29", SessionStatus::Running);
    attempt_three.session_name = "jade-main-29-attempt-3".into();
    attempt_three.pane_target = "jade-main-29-attempt-3".into();
    attempt_three.log_path = temp.path().join("attempt-3.log");
    attempt_three.attempt = 3;
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![attempt_two, attempt_three],
        },
    )
    .unwrap();
    let mut state = active_runtime_state("#29");
    state.backend = "tmux".into();
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("jade-main-29-attempt-3".into());
    state.updated_at_ms = Some(1_000);

    let summary = run_loop_resume_preflight_many(
        &tracker,
        &config,
        &[state],
        config.codex.stall_timeout_ms + 2_000,
        true,
    )
    .unwrap();

    assert_eq!(summary.active_main_workers, 1);
    assert_eq!(summary.recoverable_states.len(), 0);
    assert_eq!(
        summary.retained_states[0].backend_session_id.as_deref(),
        Some("jade-main-29-attempt-2")
    );
}

#[cfg(unix)]
#[test]
fn resume_preflight_many_recovers_registry_only_unavailable_tmux_session() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_unavailable_script(temp.path());
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("registry_session_unavailable"));
}

#[cfg(unix)]
#[test]
fn resume_preflight_many_recovers_runtime_state_unavailable_tmux_session() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    config.tmux.command = fake_tmux_unavailable_script(temp.path());
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut record = main_tmux_session_record("#29", SessionStatus::Running);
    record.session_name = "jade-main-29-attempt-1".into();
    record.pane_target = "jade-main-29-attempt-1".into();
    record.log_path = temp.path().join("session.log");
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![record],
        },
    )
    .unwrap();
    let mut state = active_runtime_state("#29");
    state.backend = "tmux".into();
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("jade-main-29-attempt-1".into());

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[state], 2_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.blocked, None);
    assert_eq!(summary.retained_states.len(), 1);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("tmux_pane_unavailable"));
}

#[cfg(unix)]
fn fake_tmux_capture_script(root: &Path, output: &str) -> String {
    use std::os::unix::fs::PermissionsExt;

    let path = root.join("fake-tmux");
    std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"${{1:-}}\" = \"capture-pane\" ]; then\ncat <<'EOF'\n{output}\nEOF\nexit 0\nfi\nexit 0\n"
            ),
        )
        .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[cfg(unix)]
fn fake_tmux_split_session_script(root: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;

    let path = root.join("fake-tmux-split-session");
    std::fs::write(
            &path,
            r#"#!/bin/sh
case "$*" in
  *attempt-2*) printf '%s\n' 'Codex' '◦ Running cargo test' '› Improve documentation in @filename'; exit 0 ;;
  *attempt-3*) printf '%s\n' 'Conversation interrupted - tell the model what to do differently.' '› Write tests for @filename'; exit 0 ;;
esac
exit 1
"#,
        )
        .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[cfg(unix)]
fn fake_tmux_unavailable_script(root: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;

    let path = root.join("fake-tmux-unavailable");
    std::fs::write(
        &path,
        "#!/bin/sh\nif [ \"${1:-}\" = \"capture-pane\" ]; then\nexit 1\nfi\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

#[test]
fn recovery_handoff_reuses_dirty_existing_issue_worktree() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.workspace.root = temp.path().join("worktrees");
    std::fs::create_dir_all(&config.workspace.root).unwrap();
    let issue = tracker_issue("In Progress");
    let mut handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let worktree = config.workspace.root.join("_29-main-agent");
    init_clean_git_workspace(&worktree);
    git_ok(
        &worktree,
        &["checkout", "-b", "feature/issue-29-runtime-state-main-loop"],
    );
    std::fs::write(worktree.join("scratch.txt"), "dirty recovery work").unwrap();
    let mut state = active_runtime_state("#29");
    state.last_event = Some("SessionRunning".into());
    state.workspace_path = Some(worktree.clone());

    let evidence = run_loop_apply_recovery_handoff(&config, &issue, &mut handoff, &state).unwrap();

    assert!(evidence
        .as_deref()
        .unwrap()
        .contains("source=runtime_state"));
    assert_eq!(handoff.workspace_path, worktree);
    assert_eq!(handoff.workspace_key, "_29-main-agent");
    assert_eq!(
        handoff.branch_name,
        "feature/issue-29-runtime-state-main-loop"
    );
}

#[test]
fn run_loop_runtime_state_uses_matching_slot_for_attempt_count() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let unrelated = active_runtime_state("#28");
    let existing = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
    let states = vec![unrelated, existing];

    let state = run_loop_runtime_state_for_issue(
        runtime_state_for_issue(&states, &issue.identifier),
        &issue,
        &config,
        "Resumed",
        &claim,
    );

    assert_eq!(state.attempt_count, 2);
    assert_eq!(
        state
            .active_issue
            .as_ref()
            .map(|issue| issue.identifier.as_str()),
        Some("#29")
    );
}

#[test]
fn resume_preflight_defers_until_retry_is_due() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    record_runtime_retry(&mut state, 1_000, 5_000, "rate limited");

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert!(matches!(
        action,
        ResumePreflightAction::RetryLater {
            due_in_ms: 4_000,
            ..
        }
    ));
}

#[test]
fn resume_preflight_continues_after_retry_is_due_even_when_old() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    record_runtime_retry(&mut state, 1_000, 5_000, "backend not ready");

    let action = run_loop_resume_preflight(
        &tracker,
        &config,
        Some(&state),
        config.codex.stall_timeout_ms + 10_000,
    )
    .unwrap();

    assert_eq!(action, ResumePreflightAction::Continue);
}

#[test]
fn resume_preflight_many_marks_due_retry_recoverable_when_requested() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    record_runtime_retry(&mut state, 1_000, 5_000, "HTTP 429 too many requests");

    let summary = run_loop_resume_preflight_many(&tracker, &config, &[state], 7_000, true).unwrap();

    assert_eq!(summary.active_main_workers, 0);
    assert_eq!(summary.recoverable_states.len(), 1);
    assert!(summary.recoverable_states[0]
        .reason
        .contains("retry_due attempt="));
}

#[test]
fn resume_preflight_detects_stalled_active_state() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
    let mut state = active_runtime_state("#29");
    state.updated_at_ms = Some(1_000);

    let action = run_loop_resume_preflight(
        &tracker,
        &config,
        Some(&state),
        config.codex.stall_timeout_ms + 2_000,
    )
    .unwrap();

    assert!(matches!(action, ResumePreflightAction::Stalled { .. }));
}

#[test]
fn resume_preflight_archives_completed_tracker_state() {
    let config = test_config();
    let tracker = MemoryTracker::new(vec![tracker_issue("Agent Review")]);
    let state = active_runtime_state("#29");

    let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

    assert_eq!(
        action,
        ResumePreflightAction::ArchiveStale {
            issue_identifier: "#29".into(),
            tracker_state: "Agent Review".into(),
            archive_reason: "tracker_state_handoff".into(),
        }
    );
}

#[test]
fn main_handoff_reconcile_completes_session_and_clears_matching_runtime_state() {
    let temp = tempfile::tempdir().unwrap();
    let config = runtime_reconcile_test_config(temp.path());
    let mut state = active_runtime_state("#338");
    state.backend = "tmux".into();
    state.backend_session_id = Some("jade-main-338-attempt-1-reconcile".into());
    state.lane = Some("main".into());
    upsert_runtime_state(&config, &state).unwrap();
    save_session_registry(
        &session_registry_path(&config),
        &jade_symphony::session_registry::SessionRegistry {
            sessions: vec![main_tmux_session_record("#338", SessionStatus::Running)],
        },
    )
    .unwrap();

    reconcile_main_handoff_runtime_state(&config, "#338", "agent_review").unwrap();

    let runtime_states = load_runtime_states(&config).unwrap();
    assert!(runtime_state_for_issue(&runtime_states, "#338").is_none());
    let registry = load_session_registry(&session_registry_path(&config)).unwrap();
    assert_eq!(registry.sessions[0].status, SessionStatus::Completed);
    assert!(registry.sessions[0].updated_at_ms > 1_000);
}

#[test]
fn main_handoff_reconcile_does_not_clear_non_main_runtime_state() {
    let temp = tempfile::tempdir().unwrap();
    let config = runtime_reconcile_test_config(temp.path());
    let mut state = active_runtime_state("#338");
    state.backend = "tmux".into();
    state.backend_session_id = Some("jade-review-338-attempt-1-review".into());
    state.lane = Some("review".into());
    upsert_runtime_state(&config, &state).unwrap();

    reconcile_main_handoff_runtime_state(&config, "#338", "agent_review").unwrap();

    let runtime_states = load_runtime_states(&config).unwrap();
    assert_eq!(
        runtime_state_for_issue(&runtime_states, "#338"),
        Some(&state)
    );
}

#[test]
fn run_loop_runtime_state_increments_same_issue_attempts() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let existing = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);

    let state =
        run_loop_runtime_state_for_issue(Some(&existing), &issue, &config, "Resumed", &claim);

    assert_eq!(state.attempt_count, 2);
    assert_eq!(
        state
            .active_issue
            .as_ref()
            .map(|issue| issue.identifier.as_str()),
        Some("#29")
    );
    assert_eq!(state.branch_name, issue.branch_name);
    assert_eq!(state.actor_role.as_deref(), Some("implementation_agent"));
    assert_eq!(state.actor_label.as_deref(), Some("Jade Symphony Agent"));
    assert_eq!(state.last_event.as_deref(), Some("Resumed"));
}

#[test]
fn run_loop_runtime_state_records_result_and_transition() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
    let result = IssueExecutionResult {
        workspace_path: PathBuf::from("/tmp/jade/issue-29"),
        backend: "dry-run".into(),
        profile_id: Some("codex-alpha".into()),
        instance_name: Some("Codex Alpha".into()),
        success: true,
        pending_session: false,
        session_id: Some("session-29".into()),
        run_id: Some(claim.run.clone()),
        backend_log_path: None,
        backend_attach_command: None,
        message: "ok".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: None,
        handoff_verification: None,
    };

    let state = run_loop_runtime_state_with_result(state, &result);
    assert_eq!(state.workspace_path, Some(result.workspace_path));
    assert_eq!(state.backend_session_id.as_deref(), Some("session-29"));
    assert_eq!(state.profile_id.as_deref(), Some("codex-alpha"));
    assert_eq!(state.actor_role.as_deref(), Some("implementation_agent"));
    assert_eq!(
        state.git_author.as_deref(),
        Some("Jade Symphony Agent <jade@example.invalid>")
    );
    assert_eq!(state.last_event.as_deref(), Some("Completed"));

    let state = run_loop_runtime_state_with_transition(
        state,
        Some("In Progress".into()),
        "agent_review",
        "main agent completed",
    );
    assert_eq!(
        state.last_transition,
        Some(RuntimeTransition {
            from: Some("In Progress".into()),
            to: "agent_review".into(),
            reason: "main agent completed".into(),
        })
    );
}

#[test]
fn run_loop_runtime_state_records_pending_tmux_session_metadata() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
    let result = IssueExecutionResult {
        workspace_path: PathBuf::from("/tmp/jade/issue-220"),
        backend: "tmux".into(),
        profile_id: None,
        instance_name: None,
        success: false,
        pending_session: true,
        session_id: Some("jade-main-220".into()),
        run_id: Some(claim.run.clone()),
        backend_log_path: Some(PathBuf::from("/tmp/jade/logs/tmux/jade-main-220.log")),
        backend_attach_command: Some("tmux attach-session -t jade-main-220".into()),
        message: "tmux session running".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Jade Symphony Agent".into(),
        git_author: None,
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::NotGitRepository,
            author: None,
            applied_keys: Vec::new(),
        },
        live_handoff: None,
        handoff_verification: None,
    };

    let state = run_loop_runtime_state_with_result(state, &result);
    let workpad = run_loop_handoff_workpad(
        &issue,
        &result,
        &run_loop_handoff_plan(&config, &issue).unwrap(),
        None,
    );

    assert_eq!(state.last_event.as_deref(), Some("SessionRunning"));
    assert_eq!(state.backend_session_id.as_deref(), Some("jade-main-220"));
    assert_eq!(
        state.backend_attach_command.as_deref(),
        Some("tmux attach-session -t jade-main-220")
    );
    assert!(workpad.contains("Session status: `running`"));
    assert!(workpad.contains("Attach command: `tmux attach-session -t jade-main-220`"));
    assert!(workpad.contains("Session log: `/tmp/jade/logs/tmux/jade-main-220.log`"));
}

#[test]
fn main_loop_reconciles_completed_pending_session_without_relaunching_backend() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = main_loop_test_config();
    config.artifacts.root = temp.path().join("artifacts");
    config.observability.logs_root = temp.path().join("logs");
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
    state.last_event = Some("SessionRunning".into());
    state.workspace_path = Some(handoff.workspace_path.clone());
    state.backend = "tmux".into();
    state.backend_session_id = Some("jade-main-29".into());
    state.backend_attach_command = Some("tmux attach-session -t jade-main-29".into());
    state.backend_log_path = Some(temp.path().join("jade-main-29.log"));

    save_session_record(
        &session_registry_path(&config),
        AgentSessionRecord {
            issue_id: Some(issue.id.clone()),
            issue_identifier: Some(issue.identifier.clone()),
            issue_title: Some(issue.title.clone()),
            lane: "main".into(),
            run_id: state.run_id.clone(),
            thread: None,
            session_source: None,
            claim_value: None,
            actor_role: state.actor_role.clone(),
            actor_label: state.actor_label.clone(),
            git_author: state.git_author.clone(),
            profile_id: state.profile_id.clone(),
            instance_name: state.instance_name.clone(),
            worktree: handoff.workspace_path.clone(),
            branch: Some(handoff.branch_name.clone()),
            backend: "codex".into(),
            session_name: "jade-main-29".into(),
            pane_target: String::new(),
            prompt_artifact_path: temp.path().join("prompt.md"),
            log_path: temp.path().join("jade-main-29.log"),
            attach_command: "tmux attach-session -t jade-main-29".into(),
            attempt: 1,
            status: SessionStatus::Completed,
            started_at_ms: 1,
            updated_at_ms: 2,
        },
    )
    .unwrap();

    let reconciliation = reconcile_pending_main_session(&config, &issue, &handoff, &state)
        .unwrap()
        .expect("expected completed session reconciliation");

    let MainSessionReconciliation::Terminal(result) = reconciliation else {
        panic!("expected terminal completed reconciliation");
    };
    assert!(result.success);
    assert!(!result.pending_session);
    assert_eq!(result.session_id.as_deref(), Some("jade-main-29"));
    assert!(result.message.contains("registry status completed"));
}

#[test]
fn main_loop_keeps_missing_pending_session_registry_active_instead_of_relaunching() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = main_loop_test_config();
    config.artifacts.root = temp.path().join("artifacts");
    let issue = tracker_issue("In Progress");
    let claim = test_claim(&issue);
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed", &claim);
    state.last_event = Some("SessionRunning".into());
    state.backend_session_id = Some("jade-main-missing".into());

    let reconciliation = reconcile_pending_main_session(&config, &issue, &handoff, &state)
        .unwrap()
        .expect("expected active missing-registry reconciliation");

    let MainSessionReconciliation::Active {
        status,
        source,
        evidence,
    } = reconciliation
    else {
        panic!("expected active reconciliation");
    };
    assert_eq!(status, "unknown");
    assert_eq!(source, "runtime");
    assert!(evidence.contains("missing from session registry"));
}

#[test]
fn run_loop_handoff_plan_uses_issue_workspace_and_branch_plan() {
    let config = test_config();
    let issue = tracker_issue("In Progress");

    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();

    assert_eq!(
        handoff.workspace_key,
        "issue-29-wire-runtime-state-persistence-into-main-loop"
    );
    assert!(handoff
        .workspace_path
        .ends_with("issue-29-wire-runtime-state-persistence-into-main-loop"));
    assert_eq!(
        handoff.branch_name,
        "feature/issue-29-wire-runtime-state-persistence-into-main-loop"
    );
    assert_eq!(
        handoff.pull_request.title,
        "#29: Wire runtime state persistence into main loop"
    );
    assert_eq!(handoff.pull_request.base_branch, "main");
}

#[test]
fn run_loop_handoff_plan_rejects_branch_for_different_issue() {
    let config = test_config();
    let mut issue = tracker_issue("In Progress");
    issue.branch_name = Some("feature/issue-99-other-work".into());

    let error = run_loop_handoff_plan(&config, &issue).unwrap_err();

    assert!(matches!(
        error,
        HandoffError::BranchIssueMismatch {
            expected_issue,
            found_issue,
            ..
        } if expected_issue == "29" && found_issue == "99"
    ));
}

#[test]
fn run_loop_handoff_workpad_records_planned_pr_evidence() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let result = IssueExecutionResult {
        workspace_path: handoff.workspace_path.clone(),
        backend: "dry-run".into(),
        profile_id: None,
        instance_name: None,
        success: true,
        pending_session: false,
        session_id: Some("session-33".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: "ok".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: Some(RunLoopLiveHandoff {
            worktree: LiveWorktreeResult {
                workspace_path: handoff.workspace_path.clone(),
                branch_name: handoff.branch_name.clone(),
                created: true,
            },
            publication: PullRequestPublication {
                branch_pushed: true,
                pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                pr_created: true,
            },
            verification: "skipped:not_configured".into(),
            project_pr_link_verified: Some(true),
            pull_request_ready: Some(PullRequestReadyStatus {
                pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                was_draft: false,
                marked_ready: false,
            }),
        }),
        handoff_verification: Some("skipped:not_configured".into()),
    };

    let workpad = run_loop_handoff_workpad(&issue, &result, &handoff, None);

    assert!(workpad.contains("### Plan"));
    assert!(workpad.contains("### Work Log"));
    assert!(workpad.contains("- [x] Read the issue contract"));
    assert!(workpad.contains("### Planned Handoff"));
    assert!(workpad.contains("Actor role: `implementation_agent`"));
    assert!(workpad.contains("Git identity: `applied:Jade Symphony Agent <jade@example.invalid>`"));
    assert!(
        workpad.contains("Workspace key: `issue-29-wire-runtime-state-persistence-into-main-loop`")
    );
    assert!(workpad
        .contains("Branch: `feature/issue-29-wire-runtime-state-persistence-into-main-loop`"));
    assert!(workpad.contains("PR title: `#29: Wire runtime state persistence into main loop`"));
    assert!(workpad.contains("Handoff verification: `skipped:not_configured`"));
    assert!(workpad.contains("Live PR: `https://github.com/Alive24/jade-symphony/pull/45`"));
}

#[test]
fn live_run_loop_handoff_records_pr_link_through_tracker() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut result = successful_live_handoff_result(&handoff);
    let adapter = RecordingAdapter::default();

    assert!(apply_live_handoff_pr_link(
        &adapter,
        &issue.identifier,
        &mut result
    ));

    assert!(result.success);
    assert_eq!(
        adapter.operations(),
        vec!["link_pr:#29:https://github.com/Alive24/jade-symphony/pull/45"]
    );
}

#[test]
fn live_run_loop_handoff_skips_link_comment_when_pr_already_visible() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut result = successful_live_handoff_result(&handoff);
    let adapter = RecordingAdapter::default();
    adapter
        .linked_pull_requests
        .borrow_mut()
        .push(jade_symphony::model::LinkedPullRequest {
            number: Some(45),
            url: Some("https://github.com/Alive24/jade-symphony/pull/45".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            ..Default::default()
        });

    assert!(apply_live_handoff_pr_link(
        &adapter,
        &issue.identifier,
        &mut result
    ));

    assert!(result.success);
    assert!(adapter.operations().is_empty());
}

#[test]
fn handoff_verification_skips_when_not_configured() {
    let config = test_config();
    let temp = tempfile::tempdir().unwrap();

    let verification = run_handoff_verification(temp.path(), &config);

    assert!(verification.success);
    assert_eq!(verification.summary, "skipped:not_configured");
}

#[test]
fn handoff_verification_runs_configured_commands() {
    let mut config = test_config();
    config.verification.commands = vec!["printf verified > verification.txt".into()];
    config.verification.timeout_ms = 5_000;
    let temp = tempfile::tempdir().unwrap();

    let verification = run_handoff_verification(temp.path(), &config);

    assert!(verification.success);
    assert_eq!(verification.summary, "passed:1 command(s)");
    assert_eq!(
        std::fs::read_to_string(temp.path().join("verification.txt")).unwrap(),
        "verified"
    );
}

#[test]
fn live_run_loop_handoff_link_failure_blocks_agent_review() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut result = successful_live_handoff_result(&handoff);
    let adapter = RecordingAdapter {
        fail_link_pr: true,
        ..Default::default()
    };

    assert!(!apply_live_handoff_pr_link(
        &adapter,
        &issue.identifier,
        &mut result
    ));

    assert!(!result.success);
    assert!(result.message.contains("handoff PR link repair failed"));
    assert_eq!(
        result
            .live_handoff
            .as_ref()
            .and_then(|handoff| handoff.project_pr_link_verified),
        Some(false)
    );
}

#[test]
fn live_run_loop_handoff_requires_verified_project_pr_linkage() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let mut result = successful_live_handoff_result(&handoff);
    let adapter = RecordingAdapter {
        confirm_link_pr: false,
        ..Default::default()
    };

    assert!(!apply_live_handoff_pr_link(
        &adapter,
        &issue.identifier,
        &mut result
    ));

    assert!(!result.success);
    assert!(result.message.contains("not Project-visible"));
    assert_eq!(
        result
            .live_handoff
            .as_ref()
            .and_then(|handoff| handoff.project_pr_link_verified),
        Some(false)
    );
}

fn successful_live_handoff_result(handoff: &IssueHandoffPlan) -> IssueExecutionResult {
    IssueExecutionResult {
        workspace_path: handoff.workspace_path.clone(),
        backend: "dry-run".into(),
        profile_id: None,
        instance_name: None,
        success: true,
        pending_session: false,
        session_id: Some("session-33".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: "ok".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: Some(RunLoopLiveHandoff {
            worktree: LiveWorktreeResult {
                workspace_path: handoff.workspace_path.clone(),
                branch_name: handoff.branch_name.clone(),
                created: true,
            },
            publication: PullRequestPublication {
                branch_pushed: true,
                pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                pr_created: true,
            },
            verification: "skipped:not_configured".into(),
            project_pr_link_verified: Some(true),
            pull_request_ready: Some(PullRequestReadyStatus {
                pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                was_draft: false,
                marked_ready: false,
            }),
        }),
        handoff_verification: Some("skipped:not_configured".into()),
    }
}

#[test]
fn handoff_verification_failure_blocks_success() {
    let mut config = test_config();
    config.verification.commands = vec!["echo nope >&2; exit 7".into()];
    config.verification.timeout_ms = 5_000;
    let temp = tempfile::tempdir().unwrap();

    let verification = run_handoff_verification(temp.path(), &config);

    assert!(!verification.success);
    assert!(verification.summary.contains("failed command=`echo nope"));
    assert!(verification.summary.contains("status 7"));
}

#[test]
fn usage_limit_pause_workpad_preserves_tracker_state_boundary() {
    let issue = tracker_issue("In Progress");
    let result = IssueExecutionResult {
        workspace_path: PathBuf::from("/tmp/jade/issue-63"),
        backend: "codex".into(),
        profile_id: None,
        instance_name: None,
        success: false,
        pending_session: false,
        session_id: Some("session-63".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: "Codex subprocess exited with status 1".into(),
        usage_limit_pause: Some(UsageLimitPause {
            classifier: "usage_limit".into(),
            evidence: "usage limit reached".into(),
        }),
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::NotGitRepository,
            author: None,
            applied_keys: Vec::new(),
        },
        live_handoff: None,
        handoff_verification: None,
    };
    let pause = result.usage_limit_pause.as_ref().unwrap();
    let workpad = run_loop_usage_limit_pause_workpad(&issue, &result, pause, 20_000);

    assert!(workpad.contains("### Usage-Limit Pause"));
    assert!(workpad.contains("Classifier: `usage_limit`"));
    assert!(workpad.contains("Tracker state was not advanced to `Agent Review`"));
    assert!(workpad.contains("Retry backoff: `20000ms`"));
}

#[test]
fn rework_transition_writes_diagnostic_before_state_change() {
    let adapter = RecordingAdapter::default();
    let issue = tracker_issue("Agent Review");
    let diagnostic = ReworkDiagnostic::validation_failure(
        issue.identifier.clone(),
        "cargo test",
        "failing test output",
    );

    let config = test_config();
    transition_issue_to_rework_with_diagnostic(&config, &adapter, &issue, &diagnostic).unwrap();

    assert_eq!(
        adapter.operations(),
        vec![
            "comment:#29".to_string(),
            "set_state:#29:rework".to_string()
        ]
    );
}

#[test]
fn rework_transition_does_not_set_state_when_timeline_comment_fails() {
    let adapter = RecordingAdapter {
        fail_comment: true,
        ..Default::default()
    };
    let issue = tracker_issue("Agent Review");
    let diagnostic = ReworkDiagnostic::validation_failure(
        issue.identifier.clone(),
        "cargo test",
        "failing test output",
    );

    let config = test_config();
    assert!(
        transition_issue_to_rework_with_diagnostic(&config, &adapter, &issue, &diagnostic).is_err()
    );
    assert!(adapter.operations().is_empty());
}

#[test]
fn merge_completion_closes_issue_after_workpad_and_done_state() {
    let adapter = RecordingAdapter::default();
    let issue = tracker_issue("Merging");
    let workpad = "## Jade Symphony Merge Run\n\n### Merge Action\n";

    let config = test_config();
    record_done_merge_lane_completion(&config, &adapter, &issue, workpad).unwrap();

    assert_eq!(
        adapter.operations(),
        vec![
            "comment:#29".to_string(),
            "set_state:#29:done".to_string(),
            "close_issue:#29".to_string()
        ]
    );
}

#[test]
fn run_loop_agent_review_handoff_blocks_missing_pr_url() {
    let config = test_config();
    let issue = tracker_issue("In Progress");
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let result = IssueExecutionResult {
        workspace_path: handoff.workspace_path.clone(),
        backend: "dry-run".into(),
        profile_id: None,
        instance_name: None,
        success: true,
        pending_session: false,
        session_id: Some("session-57".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: "ok".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: None,
        handoff_verification: None,
    };

    let workpad = run_loop_handoff_workpad(&issue, &result, &handoff, None);
    let evidence =
        run_loop_agent_review_handoff_evidence(&issue, &result, &handoff, Some(&workpad));
    let report = evaluate_agent_review_handoff(&evidence);

    assert!(!report.is_ready());
    assert_eq!(report.target_state.as_deref(), Some("need_human_input"));
    assert!(evidence
        .no_pr_blocker
        .unwrap()
        .contains("No pull request URL"));
}

#[test]
fn run_loop_agent_review_handoff_passes_with_pr_url() {
    let config = test_config();
    let mut issue = tracker_issue("In Progress");
    issue
        .linked_pull_requests
        .push(jade_symphony::model::LinkedPullRequest {
            id: Some("PR_57".into()),
            number: Some(57),
            url: Some("https://github.com/Alive24/jade-symphony/pull/57".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            ..Default::default()
        });
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let result = IssueExecutionResult {
        workspace_path: handoff.workspace_path.clone(),
        backend: "dry-run".into(),
        profile_id: None,
        instance_name: None,
        success: true,
        pending_session: false,
        session_id: Some("session-57".into()),
        run_id: None,
        backend_log_path: None,
        backend_attach_command: None,
        message: "ok".into(),
        usage_limit_pause: None,
        prompt_artifact_path: None,
        actor_role: "implementation_agent".into(),
        actor_label: "Jade Symphony Agent".into(),
        git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
        git_identity: GitIdentityApplyResult {
            status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
            author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            applied_keys: vec!["user.name".into(), "user.email".into()],
        },
        live_handoff: None,
        handoff_verification: None,
    };

    let workpad = run_loop_handoff_workpad(&issue, &result, &handoff, None);
    let evidence =
        run_loop_agent_review_handoff_evidence(&issue, &result, &handoff, Some(&workpad));
    let report = evaluate_agent_review_handoff(&evidence);

    assert!(report.is_ready());
    assert_eq!(report.target_state.as_deref(), Some("agent_review"));
    assert_eq!(
        evidence.pull_request_url.as_deref(),
        Some("https://github.com/Alive24/jade-symphony/pull/57")
    );
}

#[test]
fn run_loop_agent_review_handoff_blocks_draft_pr_and_missing_workpad_evidence() {
    let config = test_config();
    let mut issue = tracker_issue("In Progress");
    issue
        .linked_pull_requests
        .push(jade_symphony::model::LinkedPullRequest {
            id: Some("PR_57".into()),
            number: Some(57),
            url: Some("https://github.com/Alive24/jade-symphony/pull/57".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            ..Default::default()
        });
    let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
    let result = successful_live_handoff_result(&handoff);

    let missing_workpad_evidence =
        run_loop_agent_review_handoff_evidence(&issue, &result, &handoff, None);
    let missing_workpad_report = evaluate_agent_review_handoff(&missing_workpad_evidence);

    assert!(!missing_workpad_report.is_ready());
    assert!(missing_workpad_report
        .missing
        .contains(&"Main Workpad `### Plan`".into()));
    assert!(missing_workpad_report
        .missing
        .contains(&"Main Workpad `### Work Log`".into()));

    let mut draft_result = successful_live_handoff_result(&handoff);
    if let Some(live_handoff) = draft_result.live_handoff.as_mut() {
        live_handoff.pull_request_ready = Some(PullRequestReadyStatus {
            pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
            was_draft: true,
            marked_ready: false,
        });
    }
    let draft_workpad = run_loop_handoff_workpad(&issue, &draft_result, &handoff, None);
    let draft_evidence = run_loop_agent_review_handoff_evidence(
        &issue,
        &draft_result,
        &handoff,
        Some(&draft_workpad),
    );
    let draft_report = evaluate_agent_review_handoff(&draft_evidence);

    assert!(!draft_report.is_ready());
    assert!(draft_report
        .missing
        .contains(&"non-draft pull request".into()));
}

#[test]
fn forge_rework_writes_content_then_evidence_then_status() {
    let config = test_config();
    let adapter = RecordingAdapter::default();
    let mut issue = tracker_issue_with_ref("#282", "Old reviewed contract", "Human Review");
    issue.description = Some(forge_contract());
    let done_main_claim = LaneClaim::active(
        "#282",
        LaneClaimLane::Main,
        LaneClaimActor::Codex,
        LaneClaimSource::Manual,
        1_779_000_900_123,
    )
    .with_state(LaneClaimState::Done);
    issue.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String(done_main_claim.render()),
    );
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue);

    forge_rework_with_adapter(
        &config,
        &adapter,
        ForgeReworkInput {
            issue_ref: "#282".into(),
            title: "Reworked contract".into(),
            markdown: forge_contract(),
            evidence: "Prior Human Review evidence is superseded by the revised contract.".into(),
            operator_confirmation: "route to Rework".into(),
            dry_run: false,
        },
    )
    .unwrap();

    assert_eq!(
        adapter.operations(),
        vec![
            "update_issue_content:#282".to_string(),
            "comment:#282".to_string(),
            "set_state:#282:rework".to_string(),
        ]
    );
    assert_eq!(
        adapter
            .get_issue("#282")
            .unwrap()
            .unwrap()
            .normalized_state(),
        "rework"
    );
}

#[test]
fn forge_rework_records_diagnostic_for_active_human_review_claims() {
    let config = test_config();
    let adapter = RecordingAdapter::default();
    let mut issue = tracker_issue_with_ref("#282", "Reviewed contract", "Human Review");
    issue.description = Some(forge_contract());
    let active_review_claim = LaneClaim::active(
        "#282",
        LaneClaimLane::Review,
        LaneClaimActor::Gemini,
        LaneClaimSource::Manual,
        1_779_000_900_123,
    );
    issue.project_fields.insert(
        "Review Agent".into(),
        serde_json::Value::String(active_review_claim.render()),
    );
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue);

    let error = forge_rework_with_adapter(
        &config,
        &adapter,
        ForgeReworkInput {
            issue_ref: "#282".into(),
            title: "Reworked contract".into(),
            markdown: forge_contract(),
            evidence: "Reviewer changed the contract.".into(),
            operator_confirmation: "route to Rework".into(),
            dry_run: false,
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("active Review Agent claim"));
    assert_eq!(adapter.operations(), vec!["comment:#282".to_string()]);
}

#[test]
fn manual_main_claim_accepts_rework() {
    let config = test_config();
    let issue = tracker_issue("Rework");

    validate_lane_claim_state(&issue, AgentSessionLaneArg::Main, &config).unwrap();
}

#[test]
fn manual_main_claim_rejects_parent_with_incomplete_native_subissues() {
    let config = test_config();
    let mut issue = tracker_issue("Todo");
    issue.project_fields.insert(
        "GitHub Native Subissues".into(),
        serde_json::json!([
            {"identifier": "#272", "project_state": "Done"},
            {"identifier": "#273", "project_state": "Agent Review"}
        ]),
    );

    let error = validate_lane_claim_state(&issue, AgentSessionLaneArg::Main, &config)
        .unwrap_err()
        .to_string();

    assert!(error.contains("blocked by incomplete native subissues"));
    assert!(error.contains("#273=Agent Review"));
}

#[test]
fn renders_strict_promotion_note_template() {
    let note = render_promotion_note(
        "#262",
        "Standardize Issue Forge Reflect promotion notes",
        &PromotionNoteInput {
            operator_confirmation: "promote it".into(),
            decisions: vec!["Use the CLI as the enforcement point.".into()],
            scope_changes: vec!["The Backlog seed became an executable Todo issue.".into()],
            dependencies_context: vec![
                "Dependencies: none; related context is non-blocking.".into()
            ],
            readback_summaries: vec![
                "Operator confirmed the dry-run preview matched the promotion intent.".into(),
            ],
        },
        &["Readback confirmed issue `#262` and Project status `Todo`.".into()],
    );

    assert!(note.contains("## Promotion Note"));
    assert!(note.contains("- Source Backlog issue: #262"));
    assert!(note.contains("- Operator confirmation: \"promote it\""));
    assert!(note.contains("## Key Operator Decisions"));
    assert!(note.contains("## Major Scope Changes From Seed"));
    assert!(note.contains("## Dependencies and Context"));
    assert!(note.contains("## Verification Readback"));
    assert!(note.contains("- Readback confirmed issue `#262` and Project status `Todo`."));
    assert!(note.contains("- Operator confirmed the dry-run preview matched the promotion intent."));
}

#[test]
fn link_pr_helper_respects_write_intent() {
    let adapter = RecordingAdapter::default();

    assert!(!link_pr_with_adapter(&adapter, "#127", "PR_128", false).unwrap());
    assert!(adapter.operations().is_empty());

    assert!(link_pr_with_adapter(&adapter, "#127", "PR_128", true).unwrap());
    assert_eq!(adapter.operations(), vec!["link_pr:#127:PR_128"]);
}

#[test]
fn link_pr_helper_skips_repair_when_project_readback_already_has_pr() {
    let adapter = RecordingAdapter::default();
    adapter
        .linked_pull_requests
        .borrow_mut()
        .push(jade_symphony::model::LinkedPullRequest {
            number: Some(128),
            url: Some("https://github.com/Alive24/jade-symphony/pull/128".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            ..Default::default()
        });

    assert!(!link_pr_with_adapter(&adapter, "#127", "PR_128", true).unwrap());
    assert!(adapter.operations().is_empty());
}

#[test]
fn validates_forge_create_contract_before_tracker_write() {
    let config = test_config();
    assert!(
        validate_forge_create_contract("Create issue", &forge_contract(), &config, &[]).is_ok()
    );

    let error =
        validate_forge_create_contract("Thin issue", "make it better", &config, &[]).unwrap_err();
    assert!(error.contains("tracker issue was not created"));
}

#[test]
fn forge_create_draft_validation_uses_intended_assignee_for_live_github() {
    let config = live_github_config(false);
    let assignees = vec!["Alive24".to_string()];

    let report = validate_forge_create_report_with_assignees(
        "Create issue",
        &forge_contract(),
        &config,
        &assignees,
    )
    .unwrap();

    assert!(report.decision.is_dispatchable());
}

#[test]
fn forge_validate_candidate_context_uses_live_issue_assignee() {
    let config = live_github_config(false);
    let assignees = vec!["Alive24".to_string()];
    let report = forge_validation_report(
        ForgeStatusArg::Todo,
        "Candidate promoted title",
        &forge_contract(),
        &config,
        &assignees,
    )
    .unwrap();
    let categories = forge_missing_categories(&report);

    assert!(report.decision.is_dispatchable());
    assert!(categories.candidate_missing.is_empty());
    assert!(categories.live_context_missing.is_empty());
}

#[test]
fn forge_validate_candidate_context_reports_unassigned_live_issue() {
    let config = live_github_config(false);
    let report = forge_validation_report(
        ForgeStatusArg::Todo,
        "Candidate promoted title",
        &forge_contract(),
        &config,
        &[],
    )
    .unwrap();
    let categories = forge_missing_categories(&report);

    assert_eq!(
        categories.live_context_missing,
        vec!["live GitHub issue assignee".to_string()]
    );
    assert!(categories.candidate_missing.is_empty());
}

#[test]
fn forge_validate_candidate_context_reports_candidate_gaps_separately() {
    let config = live_github_config(false);
    let assignees = vec!["Alive24".to_string()];
    let report = forge_validation_report(
        ForgeStatusArg::Todo,
        "Thin issue",
        "make forge better",
        &config,
        &assignees,
    )
    .unwrap();
    let categories = forge_missing_categories(&report);

    assert!(!categories.candidate_missing.is_empty());
    assert!(categories.live_context_missing.is_empty());
}

#[test]
fn forge_create_live_github_requires_assignee_before_creation() {
    let config = live_github_config(false);

    let error = validate_forge_create_contract("Create issue", &forge_contract(), &config, &[])
        .unwrap_err();

    assert!(error.contains("tracker issue was not created"));
    assert!(forge_create_requires_assignee(
        &config,
        ForgeStatusArg::Todo
    ));
    assert!(!forge_create_requires_assignee(
        &config,
        ForgeStatusArg::Backlog
    ));
}

#[test]
fn forge_create_entrypoint_rejects_live_github_without_assignee() {
    let temp = tempfile::tempdir().unwrap();
    let workflow_path = temp.path().join("WORKFLOW.md");
    std::fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\n  assignee_filter:\n    allow_unassigned: false\nobservability:\n  logs_root: log\n---\nPrompt",
        )
        .unwrap();

    let error = forge_create(ForgeCreateOptions {
        workflow_path,
        title: "Create issue".into(),
        markdown: forge_contract(),
        status: ForgeStatusArg::Todo,
        project: None,
        project_fields: Vec::new(),
        assignees: Vec::new(),
        write: true,
        dry_run: false,
    })
    .unwrap_err()
    .to_string();

    assert_eq!(
        error,
        "forge create --status Todo requires --assignee for live GitHub issue creation"
    );
}

#[test]
fn forge_create_duplicate_title_match_normalizes_case_and_spacing() {
    let mut issue = tracker_issue("Todo");
    issue.identifier = "#143".into();
    issue.title = "Guard Issue Forge against duplicate tracker titles".into();
    let issues = [issue];

    let duplicate = find_duplicate_issue_title(
        &issues,
        "  guard   issue forge AGAINST duplicate tracker titles  ",
    )
    .unwrap();

    assert_eq!(duplicate.identifier, "#143");
}

#[test]
fn forge_create_blocks_duplicate_tracker_title_before_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_path = temp.path().join("issues.json");
    let workflow_path = temp.path().join("WORKFLOW.md");
    let mut existing = tracker_issue("Todo");
    existing.identifier = "#143".into();
    existing.title = "Create issue".into();
    existing.url = Some("https://github.com/Alive24/jade-symphony/issues/143".into());
    std::fs::write(
        &fixture_path,
        serde_json::to_string(&vec![existing]).unwrap(),
    )
    .unwrap();
    std::fs::write(
            &workflow_path,
            format!(
                "---\ntracker:\n  kind: memory\n  fixture_path: {}\nobservability:\n  logs_root: log\n---\nPrompt",
                fixture_path.display()
            ),
        )
        .unwrap();

    let error = forge_create(ForgeCreateOptions {
        workflow_path,
        title: "Create issue".into(),
        markdown: forge_contract(),
        status: ForgeStatusArg::Todo,
        project: None,
        project_fields: Vec::new(),
        assignees: Vec::new(),
        write: true,
        dry_run: false,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("duplicate tracker issue title detected"));
    assert!(error.contains("#143"));
    assert!(error.contains("https://github.com/Alive24/jade-symphony/issues/143"));
}

#[test]
fn forge_create_can_use_memory_tracker_adapter() {
    let temp = tempfile::tempdir().unwrap();
    let workflow_path = temp.path().join("WORKFLOW.md");
    std::fs::write(
        &workflow_path,
        "---\ntracker:\n  kind: memory\nobservability:\n  logs_root: log\n---\nPrompt",
    )
    .unwrap();

    forge_create(ForgeCreateOptions {
        workflow_path,
        title: "Create issue".into(),
        markdown: forge_contract(),
        status: ForgeStatusArg::Todo,
        project: None,
        project_fields: Vec::new(),
        assignees: Vec::new(),
        write: true,
        dry_run: false,
    })
    .unwrap();
}

#[test]
fn forge_create_write_initializes_backlog_without_status_transition() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.observability.logs_root = temp.path().join("logs");
    let adapter = RecordingAdapter::default();

    let create_result = write_forge_created_issue(
        &config,
        &adapter,
        ForgeCreateWriteInput {
            title: "Create Backlog seed".into(),
            markdown: forge_contract(),
            assignees: Vec::new(),
            status: ForgeStatusArg::Backlog,
            project_label: "test project",
            project_fields: &[],
        },
    )
    .unwrap();

    assert_eq!(create_result.issue_id, "dry-run:Create Backlog seed");
    assert_eq!(
        adapter.operations(),
        vec![
            "create_issue:dry-run:Create Backlog seed".to_string(),
            "add_project:dry-run:Create Backlog seed:backlog".to_string(),
        ]
    );
    assert_eq!(
        adapter
            .get_issue(&create_result.issue_id)
            .unwrap()
            .unwrap()
            .normalized_state(),
        "backlog"
    );
}

#[test]
fn forge_create_success_reports_readback_metadata_when_available() {
    let mut issue = tracker_issue_with_ref("#305", "Created issue", "Backlog");
    issue.id = "I_kwDOSZP6c88AAAABC".into();
    issue.url = Some("https://github.com/Alive24/jade-symphony/issues/305".into());
    issue
        .project_fields
        .insert("Status".into(), "Backlog".into());

    let output = render_forge_create_success(
        &ForgeCreateResult {
            issue_id: issue.id.clone(),
            readback: Some(issue),
        },
        ForgeStatusArg::Backlog,
        0,
    );

    assert_eq!(
            output,
            "forge_create=ok issue_id=I_kwDOSZP6c88AAAABC issue=#305 url=https://github.com/Alive24/jade-symphony/issues/305 status=Backlog project_status=Backlog project_fields=0"
        );
}

#[test]
fn forge_create_success_omits_unavailable_issue_metadata() {
    let output = render_forge_create_success(
        &ForgeCreateResult {
            issue_id: "memory:Create issue".into(),
            readback: None,
        },
        ForgeStatusArg::Todo,
        2,
    );

    assert_eq!(
        output,
        "forge_create=ok issue_id=memory:Create issue status=Todo project_fields=2"
    );
}

#[test]
fn forge_create_readback_failure_reports_known_issue_location() {
    let adapter = RecordingAdapter::default();
    let mut issue = tracker_issue_with_ref("#305", "Created issue", "Need to Clarify");
    issue.id = "I_kwDOSZP6c88AAAABC".into();
    issue.url = Some("https://github.com/Alive24/jade-symphony/issues/305".into());
    adapter
        .issues
        .borrow_mut()
        .insert(issue.id.clone(), issue.clone());

    let error = verify_forge_created_issue_status(&adapter, &issue.id, ForgeStatusArg::Backlog)
        .unwrap_err()
        .to_string();

    assert!(error.contains("issue_id=I_kwDOSZP6c88AAAABC"));
    assert!(error.contains("issue=#305"));
    assert!(error.contains("url=https://github.com/Alive24/jade-symphony/issues/305"));
}

#[test]
fn no_dispatch_sleeps_without_iteration_limit() {
    let options = RunLoopOptions {
        workflow_path: PathBuf::from("WORKFLOW.md"),
        max_iterations: None,
        once: false,
        max_concurrent: None,
        write: false,
        recover: false,
        display: DisplayMode::Plain,
    };

    assert_eq!(
        no_dispatch_action(options.iteration_limit(), 250),
        NoDispatchAction::SleepAndContinue { delay_ms: 250 }
    );
}

#[test]
fn run_loop_write_mode_rejects_dry_run_backend_before_runtime_writes() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().join("workspaces");
    let logs_root = temp.path().join("logs");
    let workflow_path = temp.path().join("WORKFLOW.md");
    std::fs::write(
            &workflow_path,
            format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {}\nobservability:\n  logs_root: {}\nmain_lane:\n  backend: dry-run\n---\nPrompt",
                workspace_root.display(),
                logs_root.display()
            ),
        )
        .unwrap();

    let error = run_loop(RunLoopOptions {
        workflow_path: workflow_path.clone(),
        max_iterations: Some(1),
        once: false,
        max_concurrent: None,
        write: true,
        recover: false,
        display: DisplayMode::Plain,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("write-mode main loop is blocked"));
    assert!(error.contains("main_lane.backend=dry-run"));
    assert!(error.contains(workflow_path.to_string_lossy().as_ref()));
    assert!(
        !workspace_root.exists(),
        "guard must fire before workspace creation"
    );
    assert!(!logs_root.exists(), "guard must fire before runtime writes");
}

#[test]
fn run_loop_dry_run_preview_allows_dry_run_backend() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().join("workspaces");
    let logs_root = temp.path().join("logs");
    let workflow_path = temp.path().join("WORKFLOW.md");
    std::fs::write(
            &workflow_path,
            format!(
                "---\ntracker:\n  kind: memory\nworkspace:\n  root: {}\nobservability:\n  logs_root: {}\nmain_lane:\n  backend: dry-run\n---\nPrompt",
                workspace_root.display(),
                logs_root.display()
            ),
        )
        .unwrap();

    run_loop(RunLoopOptions {
        workflow_path,
        max_iterations: Some(1),
        once: false,
        max_concurrent: None,
        write: false,
        recover: false,
        display: DisplayMode::Plain,
    })
    .unwrap();
}

#[test]
fn no_dispatch_stops_for_bounded_write_loop() {
    let options = RunLoopOptions {
        workflow_path: PathBuf::from("WORKFLOW.md"),
        max_iterations: Some(2),
        once: false,
        max_concurrent: None,
        write: true,
        recover: false,
        display: DisplayMode::Plain,
    };

    assert_eq!(
        no_dispatch_action(options.iteration_limit(), 250),
        NoDispatchAction::Stop {
            reason: "no_dispatchable_issue"
        }
    );
}

#[test]
fn no_dispatch_sleeps_for_unbounded_write_loop() {
    let options = RunLoopOptions {
        workflow_path: PathBuf::from("WORKFLOW.md"),
        max_iterations: None,
        once: false,
        max_concurrent: None,
        write: true,
        recover: false,
        display: DisplayMode::Plain,
    };

    assert_eq!(
        no_dispatch_action(options.iteration_limit(), 250),
        NoDispatchAction::SleepAndContinue { delay_ms: 250 }
    );
}
