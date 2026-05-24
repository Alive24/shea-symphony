use super::*;

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
        retrying_count: 0,
        active_issues: Vec::new(),
        retrying: Vec::new(),
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
    let adapter = shea_symphony::tracker::MemoryTracker::new(issues.clone());
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
        .push(shea_symphony::model::LinkedPullRequest {
            number: Some(339),
            url: Some("https://github.com/Alive24/shea-symphony/pull/339".into()),
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
        .push(shea_symphony::model::LinkedPullRequest {
            number: Some(339),
            url: Some("https://github.com/Alive24/shea-symphony/pull/339".into()),
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
    let adapter = shea_symphony::tracker::MemoryTracker::new(issues.clone());
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
