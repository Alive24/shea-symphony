use super::*;

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
        .push(shea_symphony::model::LinkedPullRequest {
            number: number.parse().ok(),
            url: Some(format!(
                "https://github.com/Alive24/shea-symphony/pull/{number}"
            )),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            head_sha: Some("review-fixture-head".into()),
            ..Default::default()
        });
    issue
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
    assert!(prompt.contains("Do not mutate the tracker, pull request"));
    assert!(prompt.contains("Return evidence in stdout only"));
    assert!(prompt.contains("Review Result: PASS"));
    assert!(prompt.contains("finding classifications only for actual findings"));
    assert!(prompt.contains("Leave routing and persistence"));
}

#[test]
fn agy_automatic_review_prompt_uses_only_structured_result_protocol() {
    let workflow = WorkflowDefinition::load(".shea/workflows/shea-symphony.md").unwrap();
    let prompt = render_automatic_review_prompt_for_backend(
        &workflow,
        &review_issue_with_ref("#282", "Headless review"),
        "agy-cli",
    )
    .unwrap();

    assert!(prompt.contains("Automatic Headless Structured Review Boundary"));
    assert!(prompt.contains("Use only the wrapper's native structured-result channel"));
    assert!(prompt.contains("do not create background tasks"));
    assert!(prompt.contains("disposable checkout"));
    assert!(prompt.contains("$SHEA_REVIEW_SCRATCH"));
    assert!(prompt.contains("$CARGO_TARGET_DIR"));
    assert!(prompt.contains("Capability: `.shea/contracts/workflow-capability.v1.md`"));
    assert!(prompt.contains("Active workflow: `.shea/workflows/shea-symphony.md`"));
    assert!(prompt.contains("`legacy-cli-v1`: `.shea/contracts/adapters/legacy-cli.v1.md`"));
    assert!(!prompt.contains("`.shea/adapters/legacy-cli.v1.md`"));
    assert!(prompt.contains("do not infer,\nrebase, or shorten paths from frontmatter"));
    assert!(!prompt.contains("Start with exactly one line"));
}

#[test]
fn structured_review_fails_before_launch_without_resolved_capability_resources() {
    let workflow = WorkflowDefinition::parse(
        "/tmp/WORKFLOW.md",
        "---\ntracker:\n  kind: memory\n---\nReview {{ issue.identifier }}",
    )
    .unwrap();

    let error = render_automatic_review_prompt_for_backend(
        &workflow,
        &review_issue_with_ref("#282", "Headless review"),
        "agy-cli",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("a resolved workflow resource closure is required"));
}

#[test]
fn structured_review_fails_before_launch_when_adapter_is_not_enabled() {
    let mut workflow = WorkflowDefinition::load(".shea/workflows/shea-symphony.md").unwrap();
    workflow
        .resource_closure
        .as_mut()
        .unwrap()
        .resources
        .retain(|resource| resource.kind != "adapter");

    let error = render_automatic_review_prompt_for_backend(
        &workflow,
        &review_issue_with_ref("#282", "Headless review"),
        "agy-cli",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("adapter `legacy-cli-v1` is outside resource closure"));
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
        None,
        &issue,
        ManualReviewWorkpadInput {
            decision: "passed",
            target_state: "human_review",
            evidence: "Gemini: pass",
            pass: true,
            current_claim_value: &claim,
            terminal_claim_value: &terminal,
        },
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
        None,
        &issue,
        ManualReviewWorkpadInput {
            decision: "not passed",
            target_state: "agent_review",
            evidence: "Gemini: inconclusive",
            pass: false,
            current_claim_value: &claim,
            terminal_claim_value: &terminal,
        },
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
        backend_session_id: None,
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
        "## Expected Outcome\n\n- [ ] Done\n\n<!-- shea-symphony-workpad -->\n## Agent Review";

    let body = canonical_issue_body_without_workpad(description);
    let updated = check_review_verified_issue_body_checkboxes(&body);

    assert_eq!(updated, "## Expected Outcome\n\n- [x] Done");
    assert!(!updated.contains("shea-symphony-workpad"));
}

#[test]
fn review_pass_updates_issue_body_checkboxes_before_human_review_transition() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.observability.logs_root = temp.path().to_path_buf();
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
        backend_session_id: None,
        report: Some(shea_symphony::review::AgentReviewReport {
            summary: Some("Review Result: PASS".into()),
            ..Default::default()
        }),
        error: None,
    };

    apply_review_result(None, &config, &adapter, "#67", &issue, &job, None, None).unwrap();

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
            "comment:#67",
            "update_issue_content:#67",
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
fn review_workspace_accepts_strong_adopted_candidate_outside_managed_root() {
    let workspace = PathBuf::from("/tmp/codex-worktrees/cb56/shea-symphony");
    let report = IssueWorkspaceReport {
        issue_ref: "#543".into(),
        title: "Make Markdown authoritative".into(),
        branch_hints: vec!["feature/issue-543-markdown-authoritative-prompts-workpads".into()],
        candidates: vec![IssueWorkspaceCandidate {
            path: workspace.clone(),
            branch: Some("feature/issue-543-markdown-authoritative-prompts-workpads".into()),
            head: Some("76c398d".into()),
            strength: WorkspaceMatchStrength::Strong,
            evidence: vec![
                WorkspaceEvidence {
                    source: "workpad".into(),
                    detail: "operator-adopted canonical worktree".into(),
                },
                WorkspaceEvidence {
                    source: "git_worktree".into(),
                    detail: "branch matches issue hint".into(),
                },
            ],
        }],
        canonical_index: Some(0),
        warnings: Vec::new(),
    };

    assert_eq!(strong_canonical_review_workspace(&report), Some(workspace));
}

#[test]
fn review_workspace_rejects_unverified_external_tracker_path() {
    let report = IssueWorkspaceReport {
        issue_ref: "#543".into(),
        title: "Make Markdown authoritative".into(),
        branch_hints: Vec::new(),
        candidates: vec![IssueWorkspaceCandidate {
            path: PathBuf::from("/tmp/not-a-repository-worktree"),
            branch: None,
            head: None,
            strength: WorkspaceMatchStrength::Strong,
            evidence: vec![WorkspaceEvidence {
                source: "workpad".into(),
                detail: "unverified tracker text".into(),
            }],
        }],
        canonical_index: Some(0),
        warnings: Vec::new(),
    };

    assert_eq!(strong_canonical_review_workspace(&report), None);
}

#[test]
fn automatic_review_prompt_delivers_snapshot_as_data_without_template_execution() {
    let workflow = WorkflowDefinition::parse(
        "/tmp/WORKFLOW.md",
        "---\ntracker:\n  kind: memory\n---\nReview {{ issue.identifier }}",
    )
    .unwrap();
    let mut issue = review_issue_with_ref("#282", "Headless review");
    issue.description =
        Some("Contract {{ issue.title }}\n<!-- shea-symphony-workpad -->\nMain evidence".into());
    issue.linked_pull_requests = vec![shea_symphony::model::LinkedPullRequest {
        head_sha: Some("abc123".into()),
        ..Default::default()
    }];
    let prompt =
        render_automatic_review_prompt_for_backend(&workflow, &issue, "claude-code").unwrap();
    let snapshot_start = prompt.rfind("\n{\n").unwrap();
    let snapshot: shea_symphony::model::TrackerIssue =
        serde_json::from_str(&prompt[snapshot_start..]).unwrap();
    assert_eq!(snapshot, issue);
    assert!(snapshot.description.unwrap().contains("{{ issue.title }}"));
}

fn publication_fixture() -> (
    tempfile::TempDir,
    RuntimeConfig,
    RecordingAdapter,
    TrackerIssue,
    ReviewJob,
) {
    let temp = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.observability.logs_root = temp.path().to_path_buf();
    let adapter = RecordingAdapter::default();
    let mut issue = review_issue_with_ref("#901", "Publication fixture");
    issue.description = Some("## Expected Outcome\n\n- [ ] Tested result".into());
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue.clone());
    let job = ReviewJob {
        id: "publication-run-901".into(),
        issue_ref: issue.identifier.clone(),
        backend: "fixture".into(),
        state: ReviewJobState::Completed,
        artifact_path: None,
        ledger_path: None,
        backend_session_id: None,
        report: Some(shea_symphony::review::AgentReviewReport {
            summary: Some("Review Result: PASS".into()),
            ..Default::default()
        }),
        error: None,
    };
    (temp, config, adapter, issue, job)
}

#[test]
fn review_publication_comment_failure_preserves_result_and_does_not_accept() {
    let (_temp, config, mut adapter, issue, job) = publication_fixture();
    adapter.fail_comment = true;
    assert!(apply_review_result(None, &config, &adapter, "901", &issue, &job, None, None).is_err());
    assert!(adapter.operations().is_empty());
    assert_eq!(
        adapter.get_issue("#901").unwrap().unwrap().state,
        "Agent Review"
    );
    let receipts = std::fs::read_dir(
        config
            .observability
            .logs_root
            .join("reviews/publications")
            .join(shea_symphony::workspace::safe_identifier("#901")),
    )
    .unwrap();
    let receipt = receipts
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipt).unwrap()).unwrap();
    assert_eq!(saved["state"], "ResultReady");
    assert_eq!(saved["job"]["id"], job.id);
    adapter.fail_comment = false;
    apply_review_result(None, &config, &adapter, "#901", &issue, &job, None, None).unwrap();
    let operations = adapter.operations();
    apply_review_result(None, &config, &adapter, "#901", &issue, &job, None, None).unwrap();
    assert_eq!(adapter.operations(), operations);
    assert_eq!(
        operations,
        [
            "comment:#901",
            "update_issue_content:#901",
            "set_state:#901:human_review"
        ]
    );
}

#[test]
fn review_publication_recovers_server_accepted_comment_and_state_without_duplicates() {
    let (_temp, config, mut adapter, issue, job) = publication_fixture();
    adapter.fail_comment_after_apply = true;
    adapter.fail_state_after_apply = true;
    apply_review_result(None, &config, &adapter, "#901", &issue, &job, None, None).unwrap();
    apply_review_result(None, &config, &adapter, "#901", &issue, &job, None, None).unwrap();
    assert_eq!(
        adapter.operations(),
        [
            "comment:#901",
            "update_issue_content:#901",
            "set_state:#901:human_review"
        ]
    );
}

#[test]
fn review_publication_refuses_changed_head_contract_state_and_claim() {
    for change in ["head", "body", "state", "claim", "closed", "draft"] {
        let (_temp, config, adapter, mut issue, job) = publication_fixture();
        let claim = LaneClaim::active(
            &issue.identifier,
            LaneClaimLane::Review,
            LaneClaimActor::Codex,
            LaneClaimSource::Manual,
            1_789_000_000_123,
        );
        issue
            .project_fields
            .insert("Review Agent".into(), claim.render().into());
        let mut changed = issue.clone();
        match change {
            "head" => changed.linked_pull_requests[0].head_sha = Some("new-head".into()),
            "body" => changed.description = Some("Changed scope".into()),
            "state" => changed.state = "Human Review".into(),
            "claim" => {
                changed
                    .project_fields
                    .insert("Review Agent".into(), "different owner".into());
            }
            "closed" => {
                changed
                    .project_fields
                    .insert("GitHub Issue State".into(), "closed".into());
            }
            "draft" => changed.linked_pull_requests[0].is_draft = Some(true),
            _ => unreachable!(),
        }
        adapter
            .issues
            .borrow_mut()
            .insert(issue.identifier.clone(), changed);
        assert!(
            apply_review_result(
                None,
                &config,
                &adapter,
                "#901",
                &issue,
                &job,
                Some(&claim),
                None
            )
            .is_err(),
            "{change}"
        );
        assert!(adapter.operations().is_empty(), "{change}");
    }
}

#[test]
fn review_publication_terminal_claim_follows_evidence_and_precedes_state() {
    let (_temp, config, adapter, mut issue, job) = publication_fixture();
    let claim = LaneClaim::active(
        &issue.identifier,
        LaneClaimLane::Review,
        LaneClaimActor::Codex,
        LaneClaimSource::Manual,
        1_789_000_000_123,
    );
    issue
        .project_fields
        .insert("Review Agent".into(), claim.render().into());
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue.clone());
    apply_review_result(
        None,
        &config,
        &adapter,
        "#901",
        &issue,
        &job,
        Some(&claim),
        None,
    )
    .unwrap();
    let operations = adapter.operations();
    assert_eq!(operations.first().unwrap(), "comment:#901");
    assert!(operations[1].contains("Review Agent"));
    assert_eq!(operations.last().unwrap(), "set_state:#901:human_review");
}

#[test]
fn review_recover_cli_preserves_explicit_write_intent() {
    assert!(matches!(
        parse(&["review", "recover", "WORKFLOW.md", "#901"]),
        Command::ReviewRecover { write: false, .. }
    ));
    assert!(matches!(
        parse(&["review", "recover", "WORKFLOW.md", "#901", "--write"]),
        Command::ReviewRecover { write: true, .. }
    ));
}

#[test]
fn review_publication_recovers_interruption_after_evidence_claim_and_checklist() {
    let (_temp, config, mut adapter, mut issue, job) = publication_fixture();
    let claim = LaneClaim::active(
        &issue.identifier,
        LaneClaimLane::Review,
        LaneClaimActor::Codex,
        LaneClaimSource::Manual,
        1_789_000_000_123,
    );
    issue
        .project_fields
        .insert("Review Agent".into(), claim.render().into());
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue.clone());
    adapter.fail_state_before_apply = true;
    assert!(apply_review_result(
        None,
        &config,
        &adapter,
        "#901",
        &issue,
        &job,
        Some(&claim),
        None
    )
    .is_err());
    assert_eq!(
        adapter.get_issue("#901").unwrap().unwrap().state,
        "Agent Review"
    );
    assert_eq!(adapter.operations().len(), 3);
    adapter.fail_state_before_apply = false;
    apply_review_result(
        None,
        &config,
        &adapter,
        "#901",
        &issue,
        &job,
        Some(&claim),
        None,
    )
    .unwrap();
    assert_eq!(adapter.operations().len(), 4);
    assert_eq!(
        adapter.operations().last().unwrap(),
        "set_state:#901:human_review"
    );
}

#[test]
fn review_publication_head_change_after_comment_cannot_accept_or_route() {
    let (_temp, config, mut adapter, issue, job) = publication_fixture();
    adapter.change_review_head_after_comment = true;
    assert!(
        apply_review_result(None, &config, &adapter, "#901", &issue, &job, None, None).is_err()
    );
    assert_eq!(adapter.operations(), ["comment:#901"]);
    assert_eq!(
        adapter.get_issue("#901").unwrap().unwrap().state,
        "Agent Review"
    );
}

#[test]
fn review_publication_does_not_duplicate_previously_visible_evidence() {
    let (_temp, config, mut adapter, mut issue, job) = publication_fixture();
    issue.description = None;
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue.clone());
    adapter.fail_state_before_apply = true;
    assert!(
        apply_review_result(None, &config, &adapter, "#901", &issue, &job, None, None).is_err()
    );
    adapter
        .issues
        .borrow_mut()
        .get_mut("#901")
        .unwrap()
        .description = None;
    adapter.fail_state_before_apply = false;
    assert!(
        apply_review_result(None, &config, &adapter, "#901", &issue, &job, None, None).is_err()
    );
    assert_eq!(adapter.operations(), ["comment:#901"]);
}
