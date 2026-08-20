use super::*;

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
        None,
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
        None,
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

fn revision_workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".shea/workflows/shea-symphony.md")
}

fn revision_issue(blocked: bool) -> TrackerIssue {
    let mut issue = tracker_issue_with_ref("#554", "Original Todo contract", "Todo");
    issue.description = Some(forge_contract());
    issue.assignees = vec!["Alive24".into()];
    issue.labels = vec!["contract".into()];
    issue.project_fields.insert(
        "GitHub Issue State".into(),
        serde_json::Value::String("OPEN".into()),
    );
    issue.project_fields.insert(
        "Unrelated Field".into(),
        serde_json::Value::String("preserve me".into()),
    );
    if blocked {
        issue.blocked_by.push(BlockerRef {
            id: Some("ISSUE_569".into()),
            identifier: Some("#569".into()),
            state: Some("closed".into()),
        });
    }
    issue
}

fn revised_contract() -> String {
    forge_contract().replace(
        "Create a validated tracker issue.",
        "Create a revised validated tracker issue.",
    )
}

fn insert_revision_issue(adapter: &RecordingAdapter, blocked: bool) {
    let issue = revision_issue(blocked);
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue);
}

fn revision_confirmation(
    config: &RuntimeConfig,
    adapter: &RecordingAdapter,
    title: &str,
    markdown: &str,
) -> String {
    prepare_forge_revision(
        &revision_workflow_path(),
        config,
        adapter,
        "#554",
        title,
        markdown,
    )
    .unwrap()
    .confirmation_token
}

fn apply_revision(
    config: &RuntimeConfig,
    adapter: &RecordingAdapter,
    title: &str,
    markdown: &str,
    confirmation: String,
) -> Result<(), Box<dyn std::error::Error>> {
    forge_revise_with_adapter(
        None,
        &revision_workflow_path(),
        config,
        adapter,
        ForgeReviseInput {
            issue_ref: "#554".into(),
            title: title.into(),
            markdown: markdown.into(),
            operator_confirmation: Some(confirmation),
            dry_run: false,
        },
    )
}

#[test]
fn forge_revise_preserves_todo_facts_for_blocked_and_unblocked_issues() {
    for blocked in [false, true] {
        let config = test_config();
        let adapter = RecordingAdapter::default();
        insert_revision_issue(&adapter, blocked);
        adapter
            .linked_pull_requests
            .borrow_mut()
            .push(LinkedPullRequest {
                number: Some(600),
                url: Some("https://github.com/Alive24/shea-symphony/pull/600".into()),
                state: Some("OPEN".into()),
                ..Default::default()
            });
        let before = adapter.get_issue("#554").unwrap().unwrap();
        let body = revised_contract();
        let confirmation = revision_confirmation(&config, &adapter, "Revised Todo contract", &body);

        apply_revision(
            &config,
            &adapter,
            "Revised Todo contract",
            &body,
            confirmation.clone(),
        )
        .unwrap();

        assert_eq!(
            adapter.operations(),
            vec!["comment:#554", "update_issue_content:#554"]
        );
        let after = adapter.get_issue("#554").unwrap().unwrap();
        assert_eq!(after.normalized_state(), "todo");
        assert_eq!(after.assignees, before.assignees);
        assert_eq!(after.labels, before.labels);
        assert_eq!(after.blocked_by, before.blocked_by);
        assert_eq!(after.project_fields, before.project_fields);
        assert_eq!(after.title, "Revised Todo contract");
        assert_eq!(after.description.as_deref(), Some(body.as_str()));

        apply_revision(
            &config,
            &adapter,
            "Revised Todo contract",
            &body,
            confirmation,
        )
        .unwrap();
        assert_eq!(
            adapter.operations(),
            vec!["comment:#554", "update_issue_content:#554"]
        );
    }
}

#[test]
fn forge_revise_dry_run_and_invalid_candidates_never_mutate() {
    let config = test_config();
    let adapter = RecordingAdapter::default();
    insert_revision_issue(&adapter, false);

    forge_revise_with_adapter(
        None,
        &revision_workflow_path(),
        &config,
        &adapter,
        ForgeReviseInput {
            issue_ref: "#554".into(),
            title: "Revised Todo contract".into(),
            markdown: revised_contract(),
            operator_confirmation: None,
            dry_run: true,
        },
    )
    .unwrap();
    assert!(adapter.operations().is_empty());

    let error =
        prepare_forge_revision(&revision_workflow_path(), &config, &adapter, "#554", "", "")
            .unwrap_err()
            .to_string();
    assert!(error.contains("replacement body failed executable issue gate"));
    assert!(adapter.operations().is_empty());
}

#[test]
fn forge_revise_rejects_non_todo_closed_and_all_invalid_claim_shapes() {
    let config = test_config();
    for setup in ["non_todo", "closed"] {
        let adapter = RecordingAdapter::default();
        let mut issue = revision_issue(false);
        if setup == "non_todo" {
            issue.state = "Backlog".into();
        } else {
            issue.project_fields.insert(
                "GitHub Issue State".into(),
                serde_json::Value::String("CLOSED".into()),
            );
        }
        adapter
            .issues
            .borrow_mut()
            .insert(issue.identifier.clone(), issue);
        assert!(prepare_forge_revision(
            &revision_workflow_path(),
            &config,
            &adapter,
            "#554",
            "Revised Todo contract",
            &revised_contract(),
        )
        .is_err());
        assert!(adapter.operations().is_empty());
    }

    for (field, lane) in [
        ("Main Agent", LaneClaimLane::Main),
        ("Review Agent", LaneClaimLane::Review),
        ("Merging Agent", LaneClaimLane::Merge),
    ] {
        for shape in ["active", "malformed", "mismatched"] {
            let adapter = RecordingAdapter::default();
            let mut issue = revision_issue(false);
            let value = match shape {
                "active" => LaneClaim::active(
                    "#554",
                    lane,
                    LaneClaimActor::Codex,
                    LaneClaimSource::Manual,
                    1_779_000_900_123,
                )
                .render(),
                "mismatched" => LaneClaim::active(
                    "#different",
                    lane,
                    LaneClaimActor::Codex,
                    LaneClaimSource::Manual,
                    1_779_000_900_123,
                )
                .with_state(LaneClaimState::Done)
                .render(),
                _ => "not-a-claim".into(),
            };
            issue
                .project_fields
                .insert(field.into(), serde_json::Value::String(value));
            adapter
                .issues
                .borrow_mut()
                .insert(issue.identifier.clone(), issue);
            let error = prepare_forge_revision(
                &revision_workflow_path(),
                &config,
                &adapter,
                "#554",
                "Revised Todo contract",
                &revised_contract(),
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains(shape), "{field} {shape}: {error}");
            assert!(adapter.operations().is_empty());
        }
    }

    let adapter = RecordingAdapter::default();
    let mut issue = revision_issue(false);
    issue
        .project_fields
        .insert("Main Agent".into(), serde_json::json!({"bad": "claim"}));
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue);
    assert!(prepare_forge_revision(
        &revision_workflow_path(),
        &config,
        &adapter,
        "#554",
        "Revised Todo contract",
        &revised_contract(),
    )
    .unwrap_err()
    .to_string()
    .contains("malformed Main Agent"));
}

#[test]
fn forge_revise_allows_terminal_matching_claim_audit_pointers() {
    let config = test_config();
    let adapter = RecordingAdapter::default();
    let mut issue = revision_issue(false);
    for (field, lane) in [
        ("Main Agent", LaneClaimLane::Main),
        ("Review Agent", LaneClaimLane::Review),
        ("Merging Agent", LaneClaimLane::Merge),
    ] {
        let claim = LaneClaim::active(
            "#554",
            lane,
            LaneClaimActor::Codex,
            LaneClaimSource::Manual,
            1_779_000_900_123,
        )
        .with_state(LaneClaimState::Done);
        issue
            .project_fields
            .insert(field.into(), serde_json::Value::String(claim.render()));
    }
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue);

    prepare_forge_revision(
        &revision_workflow_path(),
        &config,
        &adapter,
        "#554",
        "Revised Todo contract",
        &revised_contract(),
    )
    .unwrap();
}

#[test]
fn forge_revise_fails_closed_on_evidence_edit_and_immediate_drift_failures() {
    let config = test_config();
    for failure in ["evidence", "edit", "ambiguous", "drift", "readback"] {
        let mut adapter = RecordingAdapter::default();
        match failure {
            "evidence" => adapter.fail_comment = true,
            "edit" => adapter.fail_update_content = true,
            "ambiguous" => adapter.ambiguous_update_content = true,
            "drift" => adapter.drift_after_comment = true,
            "readback" => adapter.drift_after_update_content = true,
            _ => unreachable!(),
        }
        insert_revision_issue(&adapter, false);
        let body = revised_contract();
        let confirmation = revision_confirmation(&config, &adapter, "Revised Todo contract", &body);
        let error = apply_revision(
            &config,
            &adapter,
            "Revised Todo contract",
            &body,
            confirmation,
        )
        .unwrap_err()
        .to_string();
        match failure {
            "evidence" => assert!(error.contains("revision_evidence")),
            "edit" => assert!(error.contains("not_applied")),
            "ambiguous" => assert!(error.contains("ambiguous")),
            "drift" => assert!(error.contains("immediate_pre_edit_recheck")),
            "readback" => assert!(error.contains("final_readback")),
            _ => unreachable!(),
        }
        if matches!(failure, "evidence" | "drift") {
            assert!(!adapter
                .operations()
                .iter()
                .any(|operation| operation.starts_with("update_issue_content")));
        }
    }
}

#[test]
fn forge_revise_recovers_an_edit_that_applied_before_transport_failure() {
    let config = test_config();
    let adapter = RecordingAdapter {
        fail_update_content_after_apply: true,
        ..Default::default()
    };
    insert_revision_issue(&adapter, false);
    let body = revised_contract();
    let confirmation = revision_confirmation(&config, &adapter, "Revised Todo contract", &body);

    apply_revision(
        &config,
        &adapter,
        "Revised Todo contract",
        &body,
        confirmation,
    )
    .unwrap();
    assert_eq!(
        adapter.get_issue("#554").unwrap().unwrap().title,
        "Revised Todo contract"
    );
}

#[test]
fn forge_revise_confirmation_rejects_source_workflow_and_template_drift() {
    let config = test_config();
    let adapter = RecordingAdapter::default();
    insert_revision_issue(&adapter, false);
    let body = revised_contract();
    let confirmation = revision_confirmation(&config, &adapter, "Revised Todo contract", &body);
    adapter.issues.borrow_mut().get_mut("#554").unwrap().title = "Concurrent source edit".into();
    let error = apply_revision(
        &config,
        &adapter,
        "Revised Todo contract",
        &body,
        confirmation,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("stopped at confirmation"));
    assert!(adapter.operations().is_empty());

    for drift in ["workflow", "template"] {
        let temp = tempfile::tempdir().unwrap();
        let workflow_path = temp.path().join("WORKFLOW.md");
        let template_path = temp.path().join("issue.md");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(".shea/template/issue/executable.md"),
            &template_path,
        )
        .unwrap();
        std::fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: memory\nissue_templates:\n  executable: issue.md\n---\nPrompt",
        )
        .unwrap();
        let workflow = WorkflowDefinition::load(&workflow_path).unwrap();
        let config = RuntimeConfig::from_workflow(&workflow, &workflow_path).unwrap();
        let adapter = RecordingAdapter::default();
        insert_revision_issue(&adapter, false);
        let prepared = prepare_forge_revision(
            &workflow_path,
            &config,
            &adapter,
            "#554",
            "Revised Todo contract",
            &body,
        )
        .unwrap();
        if drift == "workflow" {
            std::fs::write(
                &workflow_path,
                "---\ntracker:\n  kind: memory\nissue_templates:\n  executable: issue.md\n---\nChanged prompt",
            )
            .unwrap();
        } else {
            let mut template = std::fs::read_to_string(&template_path).unwrap();
            template.push_str("\n{% comment %}changed{% endcomment %}\n");
            std::fs::write(&template_path, template).unwrap();
        }
        let error = forge_revise_with_adapter(
            Some(&workflow),
            &workflow_path,
            &config,
            &adapter,
            ForgeReviseInput {
                issue_ref: "#554".into(),
                title: "Revised Todo contract".into(),
                markdown: body.clone(),
                operator_confirmation: Some(prepared.confirmation_token),
                dry_run: false,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("stopped at confirmation"),
            "{drift}: {error}"
        );
        assert!(adapter.operations().is_empty());
    }
}

#[test]
fn project_set_state_dry_run_is_truthful_and_mutation_free() {
    let config = test_config();
    let adapter = RecordingAdapter::default();
    let issue = tracker_issue_with_ref("#554", "State preview", "Todo");
    adapter
        .issues
        .borrow_mut()
        .insert(issue.identifier.clone(), issue);

    set_state_with_adapter(&config, &adapter, "#554", "Todo", false, true).unwrap();
    set_state_with_adapter(&config, &adapter, "#554", "Agent Review", false, true).unwrap();
    assert!(
        set_state_with_adapter(&config, &adapter, "#554", "Human Review", false, true,).is_err()
    );
    assert!(set_state_with_adapter(
        &config,
        &adapter,
        "#554",
        "not-a-configured-state",
        false,
        true,
    )
    .is_err());
    assert!(adapter.operations().is_empty());
    assert_eq!(
        adapter
            .get_issue("#554")
            .unwrap()
            .unwrap()
            .normalized_state(),
        "todo"
    );
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
        "Standardize Issue Forge promotion notes",
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
        .push(shea_symphony::model::LinkedPullRequest {
            number: Some(128),
            url: Some("https://github.com/Alive24/shea-symphony/pull/128".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            source: shea_symphony::model::LinkedPullRequestSource::GithubNative,
            ..Default::default()
        });

    assert!(!link_pr_with_adapter(&adapter, "#127", "PR_128", true).unwrap());
    assert!(adapter.operations().is_empty());
}

#[test]
fn link_pr_helper_does_not_treat_fallback_pr_readback_as_native() {
    let adapter = RecordingAdapter::default();
    adapter
        .linked_pull_requests
        .borrow_mut()
        .push(shea_symphony::model::LinkedPullRequest {
            number: Some(128),
            url: Some("https://github.com/Alive24/shea-symphony/pull/128".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            source: shea_symphony::model::LinkedPullRequestSource::FallbackDiagnostic,
            ..Default::default()
        });

    assert!(link_pr_with_adapter(&adapter, "#127", "PR_128", true).unwrap());
    assert_eq!(adapter.operations(), vec!["link_pr:#127:PR_128"]);
}

#[test]
fn validates_forge_create_contract_before_tracker_write() {
    let config = test_config();
    assert!(
        validate_forge_create_contract("Create issue", &forge_contract(), &config, &[]).is_ok()
    );

    let report =
        validate_forge_create_contract("Thin issue", "make it better", &config, &[]).unwrap();
    assert!(report.decision.is_dispatchable());
}

#[test]
fn forge_create_draft_validation_uses_intended_assignee_for_live_github() {
    let config = live_github_config();
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
    let config = live_github_config();
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
    let config = live_github_config();
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
    let config = live_github_config();
    let assignees = vec!["Alive24".to_string()];
    let report = forge_validation_report(
        ForgeStatusArg::Todo,
        "Thin issue",
        "{{ unresolved_candidate_input }}",
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
    let config = live_github_config();

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
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 9\n  assignee_filter:\n    additional_assignees: []\nobservability:\n  logs_root: log\n---\nPrompt",
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
        relationships: ForgeRelationshipPlan::default(),
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
    existing.url = Some("https://github.com/Alive24/shea-symphony/issues/143".into());
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
        relationships: ForgeRelationshipPlan::default(),
        write: true,
        dry_run: false,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("duplicate tracker issue title detected"));
    assert!(error.contains("#143"));
    assert!(error.contains("https://github.com/Alive24/shea-symphony/issues/143"));
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
        relationships: ForgeRelationshipPlan::default(),
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
            relationships: &ForgeRelationshipPlan::default(),
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
fn forge_relationship_parent_records_integration_branch_evidence() {
    let config = test_config();
    let adapter = RecordingAdapter::default();
    let mut parent = tracker_issue_with_ref(
        "#405",
        "Make Autoloop lanes independently throughput-oriented",
        "Todo",
    );
    parent.description = Some("## Issue Setup\n\n- UAT Required: Yes".into());
    adapter
        .issues
        .borrow_mut()
        .insert(parent.identifier.clone(), parent);
    let child = tracker_issue_with_ref("#410", "Show independent lane throughput in Tauri", "Todo");
    adapter
        .issues
        .borrow_mut()
        .insert(child.identifier.clone(), child);

    let readbacks = apply_forge_relationship_plan(
        &config,
        &adapter,
        "#410",
        &ForgeRelationshipPlan {
            blocked_by: Vec::new(),
            parent: Some("#405".into()),
        },
    )
    .unwrap();

    assert_eq!(
        adapter.operations(),
        vec![
            "add_subissue:#405:#410".to_string(),
            "workpad:#405".to_string()
        ]
    );
    assert!(readbacks.iter().any(|readback| readback.contains(
        "parent integration branch `integration/issue-405-make-autoloop-lanes-independently-throughput-oriented` recorded"
    )));
    let parent = adapter.get_issue("#405").unwrap().unwrap();
    let description = parent.description.unwrap();
    assert!(description.contains("### Recovery / Rework"));
    assert!(description.contains("Parent issue:"));
    assert!(description.contains("- First observed subissue: #410"));
    assert!(description.contains(
        "- Parent integration branch: `integration/issue-405-make-autoloop-lanes-independently-throughput-oriented`"
    ));
}

fn test_promotion_note() -> PromotionNoteInput {
    PromotionNoteInput {
        operator_confirmation: "promote it".into(),
        decisions: vec!["Promote the Backlog seed through Forge.".into()],
        scope_changes: vec!["Backlog seed becomes an executable Todo issue.".into()],
        dependencies_context: vec!["Relationship requirements are explicit.".into()],
        readback_summaries: vec!["Operator reviewed the dry-run preview.".into()],
    }
}

fn memory_workflow_with_backlog_issue() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let fixture_path = temp.path().join("issues.json");
    let workflow_path = temp.path().join("WORKFLOW.md");
    let mut issue = tracker_issue_with_ref("#360", "Backlog child seed", "Backlog");
    issue.assignees = vec!["Alive24".into()];
    std::fs::write(&fixture_path, serde_json::to_string(&vec![issue]).unwrap()).unwrap();
    std::fs::write(
        &workflow_path,
        format!(
            "---\ntracker:\n  kind: memory\n  fixture_path: {}\nobservability:\n  logs_root: log\n---\nPrompt",
            fixture_path.display()
        ),
    )
    .unwrap();
    (temp, workflow_path)
}

#[test]
fn disabled_semantic_gate_does_not_parse_dependency_prose_in_rust() {
    let (_temp, workflow_path) = memory_workflow_with_backlog_issue();
    let body = forge_contract().replace(
        "- UAT Required: No",
        "- UAT Required: No\n- Dependencies: Blocked By: #358 must finish before this issue dispatches.",
    );

    forge_promote(ForgePromoteInput {
        workflow_path,
        issue_ref: "#360".into(),
        title: "Promoted child".into(),
        markdown: body,
        promotion_note: test_promotion_note(),
        relationships: ForgeRelationshipPlan::default(),
        write: false,
        dry_run: true,
    })
    .unwrap();
}

#[test]
fn forge_todo_promotion_accepts_issue_setup_dependencies_none() {
    let (_temp, workflow_path) = memory_workflow_with_backlog_issue();
    let body = forge_contract().replace(
        "- UAT Required: No",
        "- UAT Required: No\n- Dependencies: None",
    );

    forge_promote(ForgePromoteInput {
        workflow_path,
        issue_ref: "#360".into(),
        title: "Promoted child".into(),
        markdown: body,
        promotion_note: test_promotion_note(),
        relationships: ForgeRelationshipPlan::default(),
        write: false,
        dry_run: true,
    })
    .unwrap();
}

#[test]
fn forge_create_success_reports_readback_metadata_when_available() {
    let mut issue = tracker_issue_with_ref("#305", "Created issue", "Backlog");
    issue.id = "I_kwDOSZP6c88AAAABC".into();
    issue.url = Some("https://github.com/Alive24/shea-symphony/issues/305".into());
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
            "forge_create=ok issue_id=I_kwDOSZP6c88AAAABC issue=#305 url=https://github.com/Alive24/shea-symphony/issues/305 status=Backlog project_status=Backlog project_fields=0"
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
    issue.url = Some("https://github.com/Alive24/shea-symphony/issues/305".into());
    adapter
        .issues
        .borrow_mut()
        .insert(issue.id.clone(), issue.clone());

    let error = verify_forge_created_issue_status(&adapter, &issue.id, ForgeStatusArg::Backlog)
        .unwrap_err()
        .to_string();

    assert!(error.contains("issue_id=I_kwDOSZP6c88AAAABC"));
    assert!(error.contains("issue=#305"));
    assert!(error.contains("url=https://github.com/Alive24/shea-symphony/issues/305"));
}
