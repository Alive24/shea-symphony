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
    let config = main_loop_test_config();
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

fn merge_lane_plan_for_issue(identifier: &str, status: &str, action: &str) -> AutopilotLanePlan {
    AutopilotLanePlan {
        lane: "merge".into(),
        status: status.into(),
        selected_issue: Some(AutopilotIssueSummary {
            identifier: identifier.into(),
            title: "Merge recovery issue".into(),
            state: "Merging".into(),
            assignees: Vec::new(),
            url: None,
            priority: None,
            pull_request: Some("https://github.com/Alive24/shea-symphony/pull/415".into()),
        }),
        proposed_action: action.into(),
        target_state: None,
        reason: "recoverable_merge_lane_work".into(),
        evidence: vec!["source=merge_lane_decision".into()],
    }
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
    let need_to_clarify = tracker_issue_with_ref(
        "#40",
        "Needs issue contract clarification",
        "Need to Clarify",
    );
    let mut human_review = tracker_issue_with_ref("#41", "Needs human approval", "Human Review");
    human_review.assignees = vec!["Alive24".into()];
    let need_human_input =
        tracker_issue_with_ref("#42", "Needs operator decision", "Need Human Input");

    let plan = test_autopilot_plan(vec![need_to_clarify, human_review, need_human_input]);

    let clarify_queue = plan
        .parked_queues
        .iter()
        .find(|queue| queue.name == "Need to Clarify")
        .unwrap();
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
    assert_eq!(clarify_queue.count, 1);
    assert_eq!(human_queue.count, 1);
    assert_eq!(input_queue.count, 1);
    assert_eq!(human_queue.issues[0].assignees, vec!["Alive24"]);
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
fn autopilot_loop_status_allows_main_recovery_runtime_blocker() {
    let mut plan = test_autopilot_plan(Vec::new());
    let mut runtime = clean_autopilot_runtime();
    runtime.runtime_state_count = 1;
    runtime.blockers = vec!["active_runtime_states=1".into()];
    runtime.evidence = vec!["runtime issue=#364 lane=main backend=codex session=none".into()];
    runtime.active_issues = vec![AutopilotActiveIssue {
        lane: "main".into(),
        identifier: "#364".into(),
        backend: "codex".into(),
        session_id: None,
    }];
    plan.runtime = runtime;
    plan.readiness.status = "blocked_by_ambiguous_lane_or_runtime_state".into();
    plan.readiness.reason =
        "Runtime/session state needs operator attention before write-mode autopilot.".into();
    plan.readiness.blockers = vec!["active_runtime_states=1".into()];

    assert_eq!(
        plan.readiness.status,
        "blocked_by_ambiguous_lane_or_runtime_state"
    );

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: true,
            poll_interval_ms: 5_000,
            main_max_concurrent: 3,
            review_max_concurrent: 2,
            merge_max_concurrent: 3,
        },
        1,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "running");
    assert_eq!(status.counts.blocked, 0);
    assert_eq!(status.counts.running, 1);
    assert!(status.blocked_reasons.is_empty());
    assert_eq!(status.active_issues[0].identifier, "#364");
}

#[test]
fn autopilot_loop_status_allows_main_recovery_with_session_attention() {
    let mut plan = test_autopilot_plan(Vec::new());
    let mut runtime = clean_autopilot_runtime();
    runtime.runtime_state_count = 1;
    runtime.session_attention_count = 1;
    runtime.blockers = vec![
        "active_runtime_states=1".into(),
        "session_attention=1".into(),
    ];
    runtime.evidence = vec!["runtime issue=#381 lane=main backend=codex session=stale".into()];
    runtime.active_issues = vec![AutopilotActiveIssue {
        lane: "main".into(),
        identifier: "#381".into(),
        backend: "codex".into(),
        session_id: Some("stale-session".into()),
    }];
    plan.runtime = runtime;
    plan.readiness.status = "blocked_by_ambiguous_lane_or_runtime_state".into();
    plan.readiness.reason =
        "Runtime/session state needs operator attention before write-mode autopilot.".into();
    plan.readiness.blockers = vec![
        "active_runtime_states=1".into(),
        "session_attention=1".into(),
    ];

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: true,
            poll_interval_ms: 5_000,
            main_max_concurrent: 3,
            review_max_concurrent: 2,
            merge_max_concurrent: 3,
        },
        6,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "running");
    assert_eq!(status.counts.blocked, 0);
    assert_eq!(status.counts.running, 1);
    assert!(status.blocked_reasons.is_empty());
    assert_eq!(status.active_issues[0].identifier, "#381");
}

#[test]
fn autopilot_loop_status_allows_registry_only_main_session_attention_recovery() {
    let mut plan = test_autopilot_plan(Vec::new());
    let mut runtime = clean_autopilot_runtime();
    runtime.session_attention_count = 2;
    runtime.blockers = vec!["session_attention=2".into()];
    runtime.evidence = vec![
        "session=thread-408-turn-1 lane=main status=stale issue=#408".into(),
        "session=thread-415-turn-1 lane=main status=stale issue=#415".into(),
    ];
    plan.runtime = runtime;
    plan.readiness.status = "blocked_by_ambiguous_lane_or_runtime_state".into();
    plan.readiness.reason =
        "Runtime/session state needs operator attention before write-mode autopilot.".into();
    plan.readiness.blockers = vec!["session_attention=2".into()];

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: true,
            poll_interval_ms: 5_000,
            main_max_concurrent: 3,
            review_max_concurrent: 2,
            merge_max_concurrent: 3,
        },
        6,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "running");
    assert_eq!(status.counts.blocked, 0);
    assert_eq!(status.counts.running, 1);
    assert!(status.blocked_reasons.is_empty());
    assert!(status.active_issues.is_empty());
}

#[test]
fn autopilot_loop_status_keeps_main_session_attention_blocked_when_recover_disabled() {
    let mut plan = test_autopilot_plan(Vec::new());
    let mut runtime = clean_autopilot_runtime();
    runtime.runtime_state_count = 1;
    runtime.session_attention_count = 1;
    runtime.blockers = vec![
        "active_runtime_states=1".into(),
        "session_attention=1".into(),
    ];
    runtime.active_issues = vec![AutopilotActiveIssue {
        lane: "main".into(),
        identifier: "#381".into(),
        backend: "codex".into(),
        session_id: Some("stale-session".into()),
    }];
    plan.runtime = runtime;
    plan.readiness.status = "blocked_by_ambiguous_lane_or_runtime_state".into();
    plan.readiness.reason =
        "Runtime/session state needs operator attention before write-mode autopilot.".into();
    plan.readiness.blockers = vec![
        "active_runtime_states=1".into(),
        "session_attention=1".into(),
    ];

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: false,
            poll_interval_ms: 5_000,
            main_max_concurrent: 3,
            review_max_concurrent: 2,
            merge_max_concurrent: 3,
        },
        6,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "blocked");
    assert_eq!(
        status.blocked_reasons,
        vec!["active_runtime_states=1", "session_attention=1"]
    );
}

#[test]
fn autopilot_loop_status_keeps_mixed_doctor_and_session_attention_blocked() {
    let mut plan = test_autopilot_plan(Vec::new());
    let mut runtime = clean_autopilot_runtime();
    runtime.runtime_state_count = 1;
    runtime.session_attention_count = 1;
    runtime.blockers = vec![
        "active_runtime_states=1".into(),
        "session_attention=1".into(),
    ];
    runtime.active_issues = vec![AutopilotActiveIssue {
        lane: "main".into(),
        identifier: "#381".into(),
        backend: "codex".into(),
        session_id: Some("stale-session".into()),
    }];
    plan.runtime = runtime;
    plan.readiness.status = "blocked_by_doctor_or_canonical_checkout".into();
    plan.readiness.reason =
        "Doctor or runtime state needs attention before write-mode autopilot.".into();
    plan.readiness.blockers = vec![
        "doctor_blockers=1".into(),
        "active_runtime_states=1".into(),
        "session_attention=1".into(),
    ];

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: true,
            poll_interval_ms: 5_000,
            main_max_concurrent: 3,
            review_max_concurrent: 2,
            merge_max_concurrent: 3,
        },
        6,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "blocked");
    assert_eq!(
        status.blocked_reasons,
        vec![
            "doctor_blockers=1",
            "active_runtime_states=1",
            "session_attention=1"
        ]
    );
}

#[test]
fn autopilot_loop_status_keeps_non_main_session_attention_blocked() {
    let mut plan = test_autopilot_plan(Vec::new());
    let mut runtime = clean_autopilot_runtime();
    runtime.runtime_state_count = 1;
    runtime.session_attention_count = 1;
    runtime.blockers = vec![
        "active_runtime_states=1".into(),
        "session_attention=1".into(),
    ];
    runtime.active_issues = vec![AutopilotActiveIssue {
        lane: "review".into(),
        identifier: "#406".into(),
        backend: "codex".into(),
        session_id: Some("stale-review-session".into()),
    }];
    plan.runtime = runtime;
    plan.readiness.status = "blocked_by_ambiguous_lane_or_runtime_state".into();
    plan.readiness.reason =
        "Runtime/session state needs operator attention before write-mode autopilot.".into();
    plan.readiness.blockers = vec![
        "active_runtime_states=1".into(),
        "session_attention=1".into(),
    ];

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: true,
            poll_interval_ms: 5_000,
            main_max_concurrent: 3,
            review_max_concurrent: 2,
            merge_max_concurrent: 3,
        },
        6,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "blocked");
    assert_eq!(
        status.blocked_reasons,
        vec!["active_runtime_states=1", "session_attention=1"]
    );
}

#[test]
fn autopilot_loop_status_allows_merge_recovery_with_issue_scoped_session_attention() {
    let mut plan = test_autopilot_plan(Vec::new());
    let mut runtime = clean_autopilot_runtime();
    runtime.session_attention_count = 3;
    runtime.blockers = vec!["session_attention=3".into()];
    runtime.evidence = vec![
        "session=thread-415-main lane=main status=failed issue=#415".into(),
        "session=thread-415-merge-1 lane=merge status=stale issue=#415".into(),
        "session=thread-415-merge-2 lane=merge status=stale issue=#415".into(),
    ];
    plan.runtime = runtime;
    plan.readiness.status = "blocked_by_ambiguous_lane_or_runtime_state".into();
    plan.readiness.reason =
        "Runtime/session state needs operator attention before write-mode autopilot.".into();
    plan.readiness.blockers = vec!["session_attention=3".into()];
    let merge = plan
        .lanes
        .iter_mut()
        .find(|lane| lane.lane == "merge")
        .unwrap();
    *merge = merge_lane_plan_for_issue("#415", "blocked", "attempt_safe_conflict_repair");

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: true,
            poll_interval_ms: 5_000,
            main_max_concurrent: 3,
            review_max_concurrent: 2,
            merge_max_concurrent: 3,
        },
        6,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "running");
    assert_eq!(status.counts.running, 1);
    assert!(status.blocked_reasons.is_empty());
    assert_eq!(status.selected_issues[0].identifier, "#415");
}

#[test]
fn autopilot_loop_status_keeps_merge_session_attention_blocked_on_issue_mismatch() {
    let mut plan = test_autopilot_plan(Vec::new());
    let mut runtime = clean_autopilot_runtime();
    runtime.session_attention_count = 1;
    runtime.blockers = vec!["session_attention=1".into()];
    runtime.evidence = vec!["session=thread-406-merge lane=merge status=stale issue=#406".into()];
    plan.runtime = runtime;
    plan.readiness.status = "blocked_by_ambiguous_lane_or_runtime_state".into();
    plan.readiness.reason =
        "Runtime/session state needs operator attention before write-mode autopilot.".into();
    plan.readiness.blockers = vec!["session_attention=1".into()];
    let merge = plan
        .lanes
        .iter_mut()
        .find(|lane| lane.lane == "merge")
        .unwrap();
    *merge = merge_lane_plan_for_issue("#415", "blocked", "attempt_safe_conflict_repair");

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: true,
            poll_interval_ms: 5_000,
            main_max_concurrent: 3,
            review_max_concurrent: 2,
            merge_max_concurrent: 3,
        },
        6,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "blocked");
    assert_eq!(
        status.blocked_reasons,
        vec!["session_attention=1", "merge:recoverable_merge_lane_work"]
    );
}

#[test]
fn autopilot_loop_status_allows_main_parent_topology_ensure() {
    let config = main_loop_test_config();
    let mut subissue = tracker_issue_with_ref("#383", "Surface write queue state", "Todo");
    subissue.description = Some(
        [
            "## Issue Setup",
            "- UAT Required: No",
            "## Issue Goal",
            "Ship the child issue.",
            "## Why Now",
            "It blocks the parent branch flow.",
            "## Issue Context",
            "Context.",
            "## Dependencies",
            "- No blocking dependencies.",
            "## Non-Negotiable Guardrails",
            "- Keep tracker writes owned by Main.",
            "## Scope",
            "### In Scope",
            "- Code.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/shea-symphony",
            "## Verification",
            "### Functional Verification",
            "- `cargo test`",
            "### Completion Criteria",
            "- Tests pass.",
        ]
        .join("\n"),
    );
    subissue
        .project_fields
        .insert("Native Parent Issue".into(), serde_json::json!("#400"));
    subissue.project_fields.insert(
        "Native Parent Title".into(),
        serde_json::json!("Harden operator trust for supervised dogfood"),
    );
    let issues = vec![subissue];
    let adapter = shea_symphony::tracker::MemoryTracker::new(issues.clone());
    let doctor = ProjectAuditReport {
        total_issues: 1,
        violations: vec![ProjectAuditViolation {
            issue_ref: "#400".into(),
            title: "Harden operator trust for supervised dogfood".into(),
            state: "Todo".into(),
            severity: AuditSeverity::Blocker,
            code: "parent_topology_missing_integration_branch".into(),
            message: "Parent issue has native subissues but no parent integration branch evidence."
                .into(),
            suggestion: "Record the parent integration branch before subissue PRs advance.".into(),
        }],
        integration_gaps: Vec::new(),
        skill_readiness_summary: None,
    };

    let plan = build_autopilot_plan_from_parts(AutopilotPlanInputs {
        workflow_path: Path::new("/tmp/WORKFLOW.md"),
        config: &config,
        adapter: &adapter,
        issues,
        doctor_report: doctor,
        canonical_checkout: clean_autopilot_canonical(),
        runtime: clean_autopilot_runtime(),
        integration_gaps: Vec::new(),
    })
    .unwrap();

    assert_eq!(
        plan.readiness.status,
        "blocked_by_doctor_or_canonical_checkout"
    );
    assert_eq!(
        plan.doctor.blocker_codes,
        vec!["parent_topology_missing_integration_branch"]
    );
    let main_lane = plan.lanes.iter().find(|lane| lane.lane == "main").unwrap();
    assert_eq!(main_lane.status, "ready", "{main_lane:?}");

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: false,
            poll_interval_ms: 5_000,
            main_max_concurrent: 1,
            review_max_concurrent: 1,
            merge_max_concurrent: 1,
        },
        1,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "running");
    assert_eq!(status.counts.blocked, 0);
    assert_eq!(status.counts.running, 1);
    assert!(status.blocked_reasons.is_empty());
    assert_eq!(status.selected_issues[0].identifier, "#383");
}

#[test]
fn autopilot_loop_status_keeps_runtime_blocker_when_recover_disabled() {
    let mut plan = test_autopilot_plan(Vec::new());
    let mut runtime = clean_autopilot_runtime();
    runtime.runtime_state_count = 1;
    runtime.blockers = vec!["active_runtime_states=1".into()];
    runtime.active_issues = vec![AutopilotActiveIssue {
        lane: "main".into(),
        identifier: "#364".into(),
        backend: "codex".into(),
        session_id: None,
    }];
    plan.runtime = runtime;
    plan.readiness.status = "blocked_by_ambiguous_lane_or_runtime_state".into();
    plan.readiness.reason =
        "Runtime/session state needs operator attention before write-mode autopilot.".into();
    plan.readiness.blockers = vec!["active_runtime_states=1".into()];

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: false,
            poll_interval_ms: 5_000,
            main_max_concurrent: 3,
            review_max_concurrent: 2,
            merge_max_concurrent: 3,
        },
        1,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "blocked");
    assert_eq!(status.blocked_reasons, vec!["active_runtime_states=1"]);
}

#[test]
fn autopilot_loop_status_keeps_ready_lanes_running_when_one_lane_is_blocked() {
    let mut plan = test_autopilot_plan(Vec::new());
    plan.readiness.status = "ready".into();
    plan.readiness.reason = "At least one lane has useful work ready.".into();
    plan.readiness.blockers = Vec::new();
    plan.lanes = vec![
        AutopilotLanePlan {
            lane: "main".into(),
            status: "ready".into(),
            selected_issue: Some(AutopilotIssueSummary {
                identifier: "#413".into(),
                title: "Document and test independent autopilot lane throughput".into(),
                state: "Todo".into(),
                assignees: Vec::new(),
                url: None,
                priority: None,
                pull_request: None,
            }),
            proposed_action: "claim_main_issue".into(),
            target_state: Some("Agent Review".into()),
            reason: "dispatchable_issue".into(),
            evidence: vec!["source=main loop dry-run selection".into()],
        },
        AutopilotLanePlan {
            lane: "review".into(),
            status: "blocked".into(),
            selected_issue: Some(AutopilotIssueSummary {
                identifier: "#410".into(),
                title: "Show independent lane throughput in Tauri".into(),
                state: "Agent Review".into(),
                assignees: Vec::new(),
                url: None,
                priority: None,
                pull_request: Some("https://github.com/Alive24/shea-symphony/pull/410".into()),
            }),
            proposed_action: "skip".into(),
            target_state: None,
            reason: "invalid_handoff:draft_pr".into(),
            evidence: vec!["source=review_run_eligibility".into()],
        },
        AutopilotLanePlan {
            lane: "merge".into(),
            status: "idle".into(),
            selected_issue: None,
            proposed_action: "idle".into(),
            target_state: None,
            reason: "no_merging_issue".into(),
            evidence: vec!["source=merge loop dry-run selection".into()],
        },
    ];

    let status = autopilot_loop_status_from_plan(
        &plan,
        AutopilotLoopSettings {
            write: true,
            dry_run: false,
            recover: true,
            poll_interval_ms: 5_000,
            main_max_concurrent: 2,
            review_max_concurrent: 1,
            merge_max_concurrent: 3,
        },
        1,
        Some(5_000),
        &[],
        false,
    );

    assert_eq!(status.phase, "running");
    assert_eq!(status.counts.running, 1);
    assert_eq!(status.counts.blocked, 1);
    assert_eq!(status.counts.idle, 1);
    assert_eq!(status.selected_issues[0].identifier, "#413");
    assert_eq!(
        status.blocked_reasons,
        vec!["review:invalid_handoff:draft_pr"]
    );
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
            .find(|queue| queue.name == "Need to Clarify")
            .unwrap()
            .issues
            .iter()
            .map(|issue| issue.identifier.as_str())
            .collect::<Vec<_>>(),
        vec!["#334"]
    );
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
