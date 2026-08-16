use super::*;
use crate::workflow::WorkflowDefinition;
use std::cell::Cell;
use std::path::Path;
use std::time::Duration;

fn issue(state: &str) -> TrackerIssue {
    TrackerIssue {
        tracker_kind: "memory".into(),
        id: "id".into(),
        item_id: None,
        identifier: "#1".into(),
        title: "Title".into(),
        description: None,
        url: None,
        state: state.into(),
        labels: vec![],
        assignees: vec![],
        priority: None,
        branch_name: None,
        linked_pull_requests: vec![],
        blocked_by: vec![],
        project_fields: Default::default(),
        created_at: None,
        updated_at: None,
    }
}

fn github_config(source: &str) -> RuntimeConfig {
    let workflow = WorkflowDefinition::parse("/tmp/WORKFLOW.md", source).unwrap();
    RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
}

fn test_status_field() -> ProjectFieldMetadata {
    ProjectFieldMetadata {
        id: "FIELD_STATUS".into(),
        name: "Status".into(),
        kind: ProjectFieldKind::SingleSelect,
        options: vec![
            ("OPT_TODO".into(), "Todo".into()),
            ("OPT_DONE".into(), "Done".into()),
        ],
        rest_id: Some(345980099),
    }
}

fn test_text_field(name: &str, id: &str, rest_id: Option<u64>) -> ProjectFieldMetadata {
    ProjectFieldMetadata {
        id: id.into(),
        name: name.into(),
        kind: ProjectFieldKind::Text,
        options: Vec::new(),
        rest_id,
    }
}

fn test_metadata(fields: Vec<ProjectFieldMetadata>) -> ProjectMetadata {
    let status = fields
        .iter()
        .find(|field| field.name == "Status")
        .cloned()
        .unwrap_or_else(test_status_field);
    ProjectMetadata {
        owner_type: ProjectV2OwnerType::User,
        project_id: "PVT_1".into(),
        status_field_id: status.id.clone(),
        status_options: status.options,
        fields,
    }
}

#[test]
fn memory_tracker_filters_by_state() {
    let tracker = MemoryTracker::new(vec![issue("Todo"), issue("Done")]);
    let found = tracker.fetch_issues_by_states(&["todo".into()]).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].state, "Todo");
}

#[test]
fn enriches_native_subissues_with_project_statuses_from_project_read() {
    let mut parent = issue("Todo");
    parent.identifier = "#243".into();
    parent.project_fields.insert(
        "GitHub Native Subissues".into(),
        serde_json::json!([
            {"identifier": "#272", "state": "closed"},
            {"identifier": "#273", "state": "open"}
        ]),
    );
    let mut done = issue("Done");
    done.identifier = "#272".into();
    let mut active = issue("Agent Review");
    active.identifier = "#273".into();
    let mut issues = vec![parent, done, active];

    enrich_native_subissue_project_statuses_from_project_read(&mut issues);

    assert_eq!(
        issues[0]
            .project_fields
            .get("Native Subissue Project States")
            .and_then(serde_json::Value::as_str),
        Some("#272=Done, #273=Agent Review")
    );
    let native = issues[0]
        .project_fields
        .get("GitHub Native Subissues")
        .and_then(serde_json::Value::as_array)
        .unwrap();
    assert_eq!(native[0]["project_state"], "Done");
    assert_eq!(native[1]["project_state"], "Agent Review");
}

#[test]
fn hydrates_missing_native_subissue_project_statuses_from_targeted_reads() {
    let mut parent = issue("Agent Review");
    parent.identifier = "#347".into();
    parent.project_fields.insert(
        "GitHub Native Subissues".into(),
        serde_json::json!([
            {"identifier": "#348", "state": "closed", "project_state": null},
            {"identifier": "#349", "state": "closed", "project_state": "missing"}
        ]),
    );
    let mut issues = vec![parent];
    let mut project_states = project_state_map(&issues);
    let mut fetched = Vec::new();

    hydrate_missing_native_subissue_project_statuses(
        &mut issues,
        &mut project_states,
        |issue_ref| {
            fetched.push(issue_ref.to_string());
            let mut child = issue("Done");
            child.identifier = issue_ref.to_string();
            Ok(Some(child))
        },
    )
    .unwrap();

    assert_eq!(fetched, vec!["#348", "#349"]);
    assert_eq!(
        issues[0]
            .project_fields
            .get("Native Subissue Project States")
            .and_then(serde_json::Value::as_str),
        Some("#348=Done, #349=Done")
    );
}

#[test]
fn finds_native_subissue_refs_that_still_need_project_status() {
    let mut parent = issue("Todo");
    parent.identifier = "#347".into();
    parent.project_fields.insert(
        "GitHub Native Subissues".into(),
        serde_json::json!([
            {"identifier": "#348", "project_state": null},
            {"identifier": "#349", "project_state": "Done"}
        ]),
    );
    parent.project_fields.insert(
        "Native Subissues".into(),
        serde_json::Value::String("#348, #350".into()),
    );

    assert_eq!(
        native_subissue_refs_missing_project_state(&parent),
        vec!["#348".to_string(), "#350".to_string()]
    );
}

#[test]
fn github_queue_scan_query_omits_rich_issue_evidence() {
    let query = github_project_query("organization", GithubProjectReadMode::QueueScan);

    assert!(query.contains("fieldValues"));
    assert!(query.contains("assignees"));
    assert!(query.contains("subIssues"));
    assert!(!query.contains("body"));
    assert!(!query.contains("closedByPullRequestsReferences"));
    assert!(!query.contains("comments(first"));
    assert!(!query.contains("recentComments"));

    let rich_query = github_project_query("organization", GithubProjectReadMode::RichEvidence);
    assert!(rich_query.contains("body"));
    assert!(rich_query.contains("closedByPullRequestsReferences"));
    assert!(rich_query.contains("comments(first"));
    assert!(rich_query.contains("recentComments"));
}

#[test]
fn queue_scan_project_response_does_not_require_body_comments_or_prs() {
    let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );
    let response = serde_json::json!({
        "data": {
            "organization": {
                "projectV2": {
                    "items": {
                        "nodes": [
                            {
                                "id": "PVTI_7",
                                "content": {
                                    "__typename": "Issue",
                                    "id": "I_7",
                                    "number": 7,
                                    "title": "Queue scan",
                                    "url": "https://github.com/Alive24/shea-symphony/issues/7",
                                    "state": "OPEN",
                                    "createdAt": "2026-05-21T00:00:00Z",
                                    "updatedAt": "2026-05-21T00:00:00Z",
                                    "labels": {"nodes": [{"name": "Tracker"}]},
                                    "assignees": {"nodes": [{"login": "Alive24"}]},
                                    "parent": null,
                                    "subIssues": {"nodes": []}
                                },
                                "fieldValues": {
                                    "nodes": [
                                        {
                                            "name": "Todo",
                                            "field": {"name": "Status"}
                                        }
                                    ]
                                }
                            }
                        ],
                        "pageInfo": {
                            "hasNextPage": false,
                            "endCursor": null
                        }
                    }
                }
            }
        }
    });

    let (issues, cursor, has_next_page) = issues_from_project_response(&response, &config)
        .expect("queue scan payload should parse without rich issue evidence");

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].identifier, "#7");
    assert_eq!(issues[0].state, "Todo");
    assert_eq!(issues[0].description, None);
    assert!(issues[0].linked_pull_requests.is_empty());
    assert_eq!(issues[0].assignees, vec!["Alive24"]);
    assert_eq!(cursor, None);
    assert!(!has_next_page);
}

#[test]
fn rich_issue_evidence_hydration_merges_body_comments_prs_and_topology() {
    let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );
    let mut issue = issue("Todo");
    issue.identifier = "#350".into();
    issue
        .project_fields
        .insert("Status".into(), serde_json::Value::String("Todo".into()));
    let content = serde_json::json!({
        "id": "I_350",
        "number": 350,
        "title": "Rich targeted read",
        "url": "https://github.com/Alive24/shea-symphony/issues/350",
        "state": "OPEN",
        "createdAt": "2026-05-21T00:00:00Z",
        "updatedAt": "2026-05-21T00:10:00Z",
        "labels": {"nodes": [{"name": "Tracker"}]},
        "assignees": {"nodes": [{"login": "Alive24"}]},
        "parent": {
            "id": "I_347",
            "number": 347,
            "title": "Parent tracker hardening",
            "state": "OPEN",
            "url": "https://github.com/Alive24/shea-symphony/issues/347"
        },
        "subIssues": {
            "nodes": [
                {
                    "id": "I_351",
                    "number": 351,
                    "title": "Sibling",
                    "state": "OPEN",
                    "url": "https://github.com/Alive24/shea-symphony/issues/351"
                }
            ]
        },
        "body": "Issue body evidence.",
        "closedByPullRequestsReferences": {
            "nodes": [
                {
                    "id": "PR_9",
                    "number": 9,
                    "url": "https://github.com/Alive24/shea-symphony/pull/9",
                    "state": "OPEN",
                    "isDraft": false,
                    "baseRefName": "integration/issue-347-github-projectv2-rest-first-tracker",
                    "headRefName": "feature/issue-350",
                    "headRefOid": "head-sha-9"
                }
            ]
        },
        "comments": {
            "nodes": [
                {"body": "<!-- shea-symphony-workpad -->\n## Shea Symphony Workpad\n\nWorkpad evidence."}
            ]
        },
        "recentComments": {
            "nodes": [
                {"body": "## Shea Symphony Agent Review Run\n\nReview pass evidence: `recorded`"}
            ]
        }
    });

    merge_github_issue_evidence(&mut issue, &content, &config).unwrap();

    let description = issue.description.as_deref().unwrap();
    assert!(description.contains("Issue body evidence."));
    assert!(description.contains("## Shea Symphony Workpad"));
    assert!(description.contains("## Shea Symphony Agent Review Run"));
    assert_eq!(issue.linked_pull_requests.len(), 1);
    assert_eq!(issue.linked_pull_requests[0].number, Some(9));
    assert_eq!(
        issue.linked_pull_requests[0].head_sha.as_deref(),
        Some("head-sha-9")
    );
    assert_eq!(
        issue
            .project_fields
            .get("Native Parent Issue")
            .and_then(serde_json::Value::as_str),
        Some("#347")
    );
    assert_eq!(
        crate::model::native_subissue_statuses(&issue)[0].identifier,
        "#351"
    );
}

#[test]
fn github_auth_mode_distinguishes_fixture_env_token_and_gh_cli() {
    let mut config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );

    config.tracker.fixture_path = Some(Path::new("issues.json").to_path_buf());
    assert_eq!(
        github_auth_mode(&config, false, || Err("unused".into())),
        GithubAuthMode::Fixture
    );

    config.tracker.fixture_path = None;
    config.tracker.api_key = Some("redacted".into());
    assert_eq!(
        github_auth_mode(&config, false, || Err("unused".into())),
        GithubAuthMode::EnvToken
    );

    config.tracker.api_key = None;
    assert_eq!(
        github_auth_mode(&config, true, || Ok(())),
        GithubAuthMode::GhCli
    );
}

#[test]
fn github_auth_gap_only_reports_missing_or_invalid_live_auth() {
    assert_eq!(github_auth_gap(GithubAuthMode::GhCli), None);
    assert_eq!(github_auth_gap(GithubAuthMode::EnvToken), None);
    assert_eq!(
        github_auth_gap(GithubAuthMode::MissingGh).as_deref(),
        Some("GitHub Project v2 live reads require the `gh` CLI on PATH.")
    );

    let gap = github_auth_gap(GithubAuthMode::Unauthenticated {
        reason: Some("invalid token".into()),
    })
    .unwrap();
    assert!(gap.contains("gh auth login"));
    assert!(gap.contains("GITHUB_TOKEN/GH_TOKEN"));
    assert!(gap.contains("invalid token"));
}

#[test]
fn project_owner_query_order_uses_explicit_user_without_org_probe() {
    let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_owner_type: user\n  project_number: 1\n---\nPrompt",
        );

    let order = project_owner_query_order(&config).unwrap();

    assert_eq!(order, vec![ProjectV2OwnerType::User]);
    let query = github_project_query(order[0].query_field(), GithubProjectReadMode::QueueScan);
    assert!(query.contains("user(login: $owner)"));
    assert!(!query.contains("organization(login: $owner)"));
}

#[test]
fn project_owner_query_order_supports_explicit_org_and_legacy_fallback() {
    let org_config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_owner_type: organization\n  project_number: 1\n---\nPrompt",
        );
    assert_eq!(
        project_owner_query_order(&org_config).unwrap(),
        vec![ProjectV2OwnerType::Organization]
    );

    let fallback_config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );
    assert_eq!(
        project_owner_query_order(&fallback_config).unwrap(),
        vec![ProjectV2OwnerType::Organization, ProjectV2OwnerType::User]
    );
}

#[test]
fn project_owner_query_error_hides_expected_owner_miss_before_real_failure() {
    let error = project_owner_query_error(
            "ProjectV2 metadata",
            vec![
                (
                    ProjectV2OwnerType::Organization,
                    TrackerError::IntegrationUnavailable(
                        "GitHub GraphQL returned errors: Could not resolve to an Organization with the login of 'Alive24'".into(),
                    ),
                ),
                (
                    ProjectV2OwnerType::User,
                    TrackerError::IntegrationUnavailable(
                        "GitHub GraphQL operation failed kind=transient_backend: HTTP 504 Gateway Timeout".into(),
                    ),
                ),
            ],
        );
    let rendered = error.to_string();

    assert!(rendered.contains("ProjectV2 metadata failed as user owner"));
    assert!(rendered.contains("HTTP 504"));
    assert!(!rendered.contains("Organization with the login"));
    assert_eq!(
        classify_project_state_error(&error),
        ProjectStateFailureKind::TransientBackend
    );
}

#[test]
fn parses_github_project_v2_issue_items() {
    let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );
    let response = serde_json::json!({
        "data": {
            "organization": {
                "projectV2": {
                    "items": {
                        "pageInfo": {
                            "hasNextPage": false,
                            "endCursor": null
                        },
                        "nodes": [
                            {
                                "id": "PVTI_1",
                                "fieldValues": {
                                    "nodes": [
                                        {
                                            "name": "Todo",
                                            "field": {"name": "Status"}
                                        },
                                        {
                                            "number": 1.0,
                                            "field": {"name": "Priority"}
                                        }
                                    ]
                                },
                                "content": {
                                    "__typename": "Issue",
                                    "id": "GHI_1",
                                    "number": 42,
                                    "title": "Implement adapter",
                                    "body": "body",
                                    "url": "https://github.com/Alive24/shea-symphony/issues/42",
                                    "state": "OPEN",
                                    "createdAt": "2026-05-10T00:00:00Z",
                                    "updatedAt": "2026-05-10T01:00:00Z",
                                    "labels": {"nodes": [{"name": "Dogfood"}]},
                                    "assignees": {"nodes": [{"login": "codex"}]},
                                    "parent": {
                                        "number": 243,
                                        "title": "Complete parent/subissue orchestration umbrella gating",
                                        "state": "OPEN",
                                        "url": "https://github.com/Alive24/shea-symphony/issues/243"
                                    },
                                    "subIssues": {
                                        "nodes": [
                                            {
                                                "number": 274,
                                                "title": "Teach lane flows about parent integration branches",
                                                "state": "OPEN",
                                                "url": "https://github.com/Alive24/shea-symphony/issues/274"
                                            }
                                        ]
                                    },
                                    "closedByPullRequestsReferences": {
                                        "nodes": [
                                            {
                                                "id": "PR_1",
                                                "number": 7,
                                                "url": "https://github.com/Alive24/shea-symphony/pull/7",
                                                "state": "OPEN",
                                                "baseRefName": "integration/issue-41-parent",
                                                "headRefName": "feature/issue-42-implement-adapter",
                                                "headRefOid": "head-sha-7"
                                            }
                                        ]
                                    },
                                    "comments": {
                                        "nodes": [
                                            {
                                                "body": "Shea Symphony linked pull request: https://github.com/Alive24/shea-symphony/pull/289"
                                            }
                                        ]
                                    }
                                }
                            },
                            {
                                "id": "PVTI_DRAFT",
                                "fieldValues": {"nodes": []},
                                "content": {"__typename": "DraftIssue"}
                            }
                        ]
                    }
                }
            }
        }
    });

    let (issues, next_cursor, has_next) = issues_from_project_response(&response, &config).unwrap();

    assert!(!has_next);
    assert_eq!(next_cursor, None);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].identifier, "#42");
    assert_eq!(issues[0].item_id.as_deref(), Some("PVTI_1"));
    assert_eq!(issues[0].state, "Todo");
    assert_eq!(issues[0].labels, vec!["dogfood"]);
    assert_eq!(issues[0].assignees, vec!["codex"]);
    assert_eq!(issues[0].priority, Some(1));
    assert_eq!(
        issues[0]
            .project_fields
            .get("GitHub Issue State")
            .and_then(serde_json::Value::as_str),
        Some("OPEN")
    );
    assert_eq!(
        issues[0]
            .project_fields
            .get("Native Parent Issue")
            .and_then(serde_json::Value::as_str),
        Some("#243")
    );
    assert_eq!(
        issues[0]
            .project_fields
            .get("Native Subissues")
            .and_then(serde_json::Value::as_str),
        Some("#274")
    );
    assert_eq!(issues[0].linked_pull_requests[0].number, Some(7));
    assert_eq!(
        issues[0].linked_pull_requests[0].head_sha.as_deref(),
        Some("head-sha-7")
    );
    assert_eq!(
        issues[0].linked_pull_requests[0].base_ref_name.as_deref(),
        Some("integration/issue-41-parent")
    );
    assert_eq!(
        issues[0]
            .project_fields
            .get("GitHub Native Parent")
            .and_then(|value| value.get("identifier"))
            .and_then(serde_json::Value::as_str),
        Some("#243")
    );
    assert_eq!(
        issues[0]
            .project_fields
            .get("GitHub Native Subissues")
            .and_then(serde_json::Value::as_array)
            .and_then(|values| values.first())
            .and_then(|value| value.get("identifier"))
            .and_then(serde_json::Value::as_str),
        Some("#274")
    );
    assert!(issues[0]
        .linked_pull_requests
        .iter()
        .any(|pr| pr.number == Some(289)));
}

#[test]
fn discovers_pull_request_urls_from_workpad_text() {
    let bodies = vec![format!(
            "{}\n- Live PR: `https://github.com/Alive24/shea-symphony/pull/98` (created: `true`)\n- Also see https://github.com/Alive24/shea-symphony/pull/100.\nShea Symphony linked pull request: 101",
            "<!-- shea-symphony-workpad -->"
        )];

    let prs = linked_pull_requests_from_workpads(&bodies, Some("Alive24"), Some("shea-symphony"));

    assert_eq!(prs.len(), 3);
    assert_eq!(
        prs[0].url.as_deref(),
        Some("https://github.com/Alive24/shea-symphony/pull/98")
    );
    assert_eq!(prs[0].number, Some(98));
    assert_eq!(prs[0].state, None);
    assert_eq!(
        prs[0].source,
        crate::model::LinkedPullRequestSource::FallbackDiagnostic
    );
    assert_eq!(
        prs[1].url.as_deref(),
        Some("https://github.com/Alive24/shea-symphony/pull/100")
    );
    assert_eq!(prs[2].number, Some(101));
    assert_eq!(
        prs[2].url.as_deref(),
        Some("https://github.com/Alive24/shea-symphony/pull/101")
    );
}

#[test]
fn merge_linked_pull_requests_deduplicates_by_url() {
    let closing_ref = LinkedPullRequest {
        id: Some("PR_98".into()),
        number: Some(98),
        url: Some("https://github.com/Alive24/shea-symphony/pull/98".into()),
        state: Some("OPEN".into()),
        is_draft: None,
        merge_state_status: None,
        review_decision: None,
        base_ref_name: None,
        head_ref_name: None,
        head_sha: None,
        source: crate::model::LinkedPullRequestSource::GithubNative,
    };
    let discovered_duplicate =
        linked_pull_request_from_url("https://github.com/Alive24/shea-symphony/pull/98");
    let discovered_new =
        linked_pull_request_from_url("https://github.com/Alive24/shea-symphony/pull/100");

    let merged = merge_linked_pull_requests(
        vec![closing_ref],
        vec![discovered_duplicate, discovered_new],
    );

    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].id.as_deref(), Some("PR_98"));
    assert_eq!(merged[0].state.as_deref(), Some("OPEN"));
    assert_eq!(
        merged[0].source,
        crate::model::LinkedPullRequestSource::GithubNative
    );
    assert_eq!(
        merged[1].url.as_deref(),
        Some("https://github.com/Alive24/shea-symphony/pull/100")
    );
}

#[test]
fn github_issue_description_includes_canonical_workpad_comment() {
    let content = serde_json::json!({
        "body": "issue body",
        "comments": {
            "nodes": [
                {"body": "ordinary comment"},
                {"body": "<!-- shea-symphony-workpad -->\n## Workpad\n\n<!-- shea-symphony-runtime-ownership -->\n### Runtime Ownership\n<!-- /shea-symphony-runtime-ownership -->"}
            ]
        }
    });

    let description =
        github_issue_description_with_workpad(&content, "<!-- shea-symphony-workpad -->").unwrap();

    assert!(description.contains("issue body"));
    assert!(description.contains("shea-symphony-runtime-ownership"));
}

#[test]
fn github_issue_description_includes_timeline_comments_for_review_evidence() {
    let content = serde_json::json!({
        "body": "issue body",
        "comments": {
            "nodes": [
                {"body": "<!-- shea-symphony-workpad -->\n## Shea Symphony Workpad"},
                {"body": "ordinary comment"}
            ]
        },
        "recentComments": {
            "nodes": [
                {"body": "ordinary recent comment"},
                {"body": "## Shea Symphony Agent Review Run\n\nReview pass evidence: `recorded`"}
            ]
        }
    });

    let description =
        github_issue_description_with_workpad(&content, "<!-- shea-symphony-workpad -->").unwrap();

    assert!(description.contains("## Shea Symphony Workpad"));
    assert!(description.contains("## Shea Symphony Agent Review Run"));
    assert!(description.contains("Review pass evidence: `recorded`"));
}

#[test]
fn filters_github_read_issues_by_status_map_and_assignees() {
    let config = github_config(
        r#"---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: shea-symphony
  project_owner: Alive24
  project_number: 1
  assignee_filter:
    source: issue_assignees
    additional_assignees:
      - codex
---
Prompt
"#,
    );

    let mut mapped_assigned = issue("Todo");
    mapped_assigned.assignees = vec!["Codex".into()];
    let mut unmapped_status = issue("Custom Status");
    unmapped_status.assignees = vec!["codex".into()];
    let mut wrong_assignee = issue("Todo");
    wrong_assignee.assignees = vec!["someone-else".into()];
    let unassigned = issue("Todo");

    let filtered = apply_github_read_filters(
        vec![mapped_assigned, unmapped_status, wrong_assignee, unassigned],
        &config,
        None,
    );

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].state, "Todo");
    assert_eq!(filtered[0].assignees, vec!["Codex"]);
}

#[test]
fn github_state_reads_can_bypass_main_assignee_filter_for_merge_lane() {
    let config = github_config(
        r#"---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: shea-symphony
  project_owner: Alive24
  project_number: 1
  assignee_filter:
    source: issue_assignees
    additional_assignees: []
---
Prompt
"#,
    );

    let unassigned_merging = issue("Merging");
    let dispatch_filtered =
        apply_github_read_filters(vec![unassigned_merging.clone()], &config, Some("codex"));
    let state_filtered = apply_github_status_filters(vec![unassigned_merging], &config);

    assert!(dispatch_filtered.is_empty());
    assert_eq!(state_filtered.len(), 1);
    assert_eq!(state_filtered[0].state, "Merging");
}

#[test]
fn empty_additional_assignees_filters_to_current_login() {
    let config = github_config(
        r#"---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: shea-symphony
  project_owner: Alive24
  project_number: 1
  assignee_filter:
    source: issue_assignees
    additional_assignees: []
---
Prompt
"#,
    );

    let mut mine = issue("Todo");
    mine.assignees = vec!["Codex".into()];
    let mut theirs = issue("Todo");
    theirs.assignees = vec!["someone-else".into()];

    let filtered =
        apply_github_read_filters(vec![mine.clone(), theirs.clone()], &config, Some("codex"));
    let no_login = apply_github_read_filters(vec![mine, theirs], &config, None);

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].assignees, vec!["Codex"]);
    assert!(no_login.is_empty());
}

#[test]
fn github_assignee_filter_allows_current_login_and_additional_assignees() {
    let filter = AssigneeFilter {
        source: "issue_assignees".into(),
        additional_assignees: vec!["teammate".into()],
    };
    let mut mine = issue("Todo");
    mine.assignees = vec!["Codex".into()];
    let mut teammate = issue("Todo");
    teammate.assignees = vec!["teammate".into()];

    assert!(issue_matches_assignee_filter(&mine, &filter, Some("codex")));
    assert!(issue_matches_assignee_filter(
        &teammate,
        &filter,
        Some("codex")
    ));
    assert!(!issue_matches_assignee_filter(
        &issue("Todo"),
        &filter,
        Some("codex")
    ));
}

#[test]
fn parses_blocker_refs_from_project_fields_when_present() {
    let mut fields = BTreeMap::new();
    fields.insert(
        "Blocked By".into(),
        serde_json::Value::String("#1, #2\nGHI_3".into()),
    );

    let blockers = blocker_refs_from_project_fields(&fields);

    assert_eq!(blockers.len(), 3);
    assert_eq!(blockers[0].identifier.as_deref(), Some("#1"));
    assert_eq!(blockers[1].identifier.as_deref(), Some("#2"));
    assert_eq!(blockers[2].identifier.as_deref(), Some("GHI_3"));
}

#[test]
fn parses_github_native_blocker_refs_from_rest_response() {
    let response = serde_json::json!([
        {
            "id": 4453853955u64,
            "node_id": "I_kwDOSZP6c88AAAABCXhrAw",
            "number": 225,
            "state": "open"
        },
        {
            "id": 4453858123u64,
            "number": 226,
            "state": "closed"
        }
    ]);

    let blockers = github_native_blocker_refs_from_response(&response, 224).unwrap();

    assert_eq!(blockers.len(), 2);
    assert_eq!(blockers[0].id.as_deref(), Some("I_kwDOSZP6c88AAAABCXhrAw"));
    assert_eq!(blockers[0].identifier.as_deref(), Some("#225"));
    assert_eq!(blockers[0].state.as_deref(), Some("open"));
    assert_eq!(blockers[1].id.as_deref(), Some("4453858123"));
    assert_eq!(blockers[1].identifier.as_deref(), Some("#226"));
    assert_eq!(blockers[1].state.as_deref(), Some("closed"));
}

#[test]
fn native_blocker_refs_enrich_project_field_blockers() {
    let mut existing = vec![BlockerRef {
        id: None,
        identifier: Some("#225".into()),
        state: None,
    }];
    let incoming = vec![
        BlockerRef {
            id: Some("I_225".into()),
            identifier: Some("#225".into()),
            state: Some("open".into()),
        },
        BlockerRef {
            id: Some("I_226".into()),
            identifier: Some("#226".into()),
            state: Some("closed".into()),
        },
    ];

    merge_blocker_refs(&mut existing, incoming);

    assert_eq!(existing.len(), 2);
    assert_eq!(existing[0].id.as_deref(), Some("I_225"));
    assert_eq!(existing[0].identifier.as_deref(), Some("#225"));
    assert_eq!(existing[0].state.as_deref(), Some("open"));
    assert_eq!(existing[1].identifier.as_deref(), Some("#226"));
}

#[test]
fn parses_project_field_assignment() {
    let assignment = ProjectFieldAssignment::parse("Capability=CLI").unwrap();

    assert_eq!(assignment.name, "Capability");
    assert_eq!(assignment.value, "CLI");
    assert!(ProjectFieldAssignment::parse("Capability").is_err());
    assert!(ProjectFieldAssignment::parse("=CLI").is_err());
}

#[test]
fn parses_project_metadata_status_options() {
    let response = serde_json::json!({
        "data": {
            "user": {
                "projectV2": {
                    "id": "PVT_1",
                    "fields": {
                        "nodes": [
                            {
                                "id": "FIELD_STATUS",
                                "name": "Status",
                                "__typename": "ProjectV2SingleSelectField",
                                "options": [
                                    {"id": "OPT_TODO", "name": "Todo"},
                                    {"id": "OPT_DONE", "name": "Done"}
                                ]
                            },
                            {
                                "id": "FIELD_CAPABILITY",
                                "name": "Capability",
                                "__typename": "ProjectV2SingleSelectField",
                                "options": [
                                    {"id": "OPT_CLI", "name": "CLI"},
                                    {"id": "OPT_TRACKER", "name": "Tracker"}
                                ]
                            },
                            {
                                "id": "FIELD_MAIN_AGENT",
                                "name": "Main Agent",
                                "__typename": "ProjectV2Field"
                            }
                        ]
                    }
                }
            }
        }
    });

    let metadata = project_metadata_from_response(&response, "Status").unwrap();
    assert_eq!(metadata.owner_type, ProjectV2OwnerType::User);
    assert_eq!(metadata.project_id, "PVT_1");
    assert_eq!(metadata.status_field_id, "FIELD_STATUS");
    assert_eq!(
        metadata.status_options,
        vec![
            ("OPT_TODO".into(), "Todo".into()),
            ("OPT_DONE".into(), "Done".into())
        ]
    );
    let capability = metadata.field("Capability").unwrap();
    assert_eq!(capability.kind, ProjectFieldKind::SingleSelect);
    assert_eq!(capability.option_id("CLI").as_deref(), Some("OPT_CLI"));
    let main_agent = metadata.field("Main Agent").unwrap();
    assert_eq!(main_agent.kind, ProjectFieldKind::Text);
}

#[test]
fn project_metadata_cache_reuses_loaded_metadata() {
    let cache = ProjectMetadataCache::default();
    let calls = Cell::new(0);
    let metadata = test_metadata(vec![test_status_field()]);

    let first = cache
        .get_or_try_init(|| {
            calls.set(calls.get() + 1);
            Ok(metadata.clone())
        })
        .unwrap();
    let second = cache
        .get_or_try_init(|| {
            calls.set(calls.get() + 1);
            Ok(test_metadata(vec![]))
        })
        .unwrap();

    assert_eq!(calls.get(), 1);
    assert_eq!(first.project_id, "PVT_1");
    assert_eq!(second.status_field_id, "FIELD_STATUS");
}

#[test]
fn project_field_lookup_refreshes_stale_metadata() {
    let cache = ProjectMetadataCache::default();
    let calls = Cell::new(0);

    let (_metadata, field) = project_field_from_metadata_with_refresh(&cache, "Main Agent", || {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            Ok(test_metadata(vec![test_status_field()]))
        } else {
            Ok(test_metadata(vec![
                test_status_field(),
                test_text_field("Main Agent", "FIELD_MAIN_AGENT", Some(347408996)),
            ]))
        }
    })
    .unwrap();

    assert_eq!(calls.get(), 2);
    assert_eq!(field.name, "Main Agent");
    assert_eq!(field.rest_id, Some(347408996));
}

#[test]
fn parses_rest_project_metadata_from_paginated_fields() {
    let project = serde_json::json!({"node_id": "PVT_1"});
    let fields = serde_json::json!([
        [
            {
                "id": 345980099,
                "node_id": "FIELD_STATUS",
                "name": "Status",
                "data_type": "single_select",
                "options": [
                    {"id": "OPT_TODO", "name": {"raw": "Todo", "html": "Todo"}},
                    {"id": "OPT_DONE", "name": {"raw": "Done", "html": "Done"}}
                ]
            }
        ],
        [
            {
                "id": 347408996,
                "node_id": "FIELD_MAIN_AGENT",
                "name": "Main Agent",
                "data_type": "text"
            },
            {
                "id": 348315440,
                "node_id": "FIELD_REVIEW_AGENT",
                "name": "Review Agent",
                "data_type": "text"
            }
        ]
    ]);

    let metadata = rest_project_metadata_from_response(
        &project,
        &fields,
        "Status",
        ProjectV2OwnerType::Organization,
    )
    .unwrap();

    assert_eq!(metadata.project_id, "PVT_1");
    assert_eq!(metadata.owner_type, ProjectV2OwnerType::Organization);
    assert_eq!(metadata.status_field_id, "FIELD_STATUS");
    assert_eq!(
        metadata.status_options[0],
        ("OPT_TODO".into(), "Todo".into())
    );
    assert_eq!(metadata.field("Status").unwrap().rest_id, Some(345980099));
    assert_eq!(
        metadata.field("Main Agent").unwrap().rest_id,
        Some(347408996)
    );
}

#[test]
fn parses_rest_project_item_overlays_from_paginated_items() {
    let response = serde_json::json!([
        [
            {
                "id": 190539790,
                "node_id": "PVTI_1",
                "content": {
                    "node_id": "I_1",
                    "number": 349
                },
                "fields": [
                    {
                        "id": 345980099,
                        "name": "Status",
                        "data_type": "single_select",
                        "value": {"id": "OPT_TODO", "name": {"raw": "Todo"}}
                    },
                    {
                        "id": 347408996,
                        "name": "Main Agent",
                        "data_type": "text",
                        "value": "v=1 lane=main"
                    },
                    {
                        "id": 347408997,
                        "name": "Parent Integration Branch",
                        "data_type": "text",
                        "value": {
                            "raw": "integration/issue-468-feedback-intake",
                            "html": "<code>integration/incorrect-html-fallback</code>"
                        }
                    }
                ]
            }
        ],
        [
            {
                "id": 190539791,
                "node_id": "PVTI_2",
                "content": {
                    "node_id": "I_2",
                    "number": 350
                },
                "fields": [
                    {
                        "id": 1,
                        "name": "Priority",
                        "data_type": "number",
                        "value": 3
                    },
                    {
                        "id": 2,
                        "name": "Target Date",
                        "data_type": "date",
                        "value": "2026-05-22"
                    },
                    {
                        "id": 3,
                        "name": "Rich Target Date",
                        "data_type": "date",
                        "value": {
                            "raw": "2026-07-19",
                            "html": "<time>2026-07-20</time>"
                        }
                    }
                ]
            }
        ]
    ]);

    let overlays = rest_project_item_overlays_from_response(&response).unwrap();

    let first = overlays.get("I_1").unwrap();
    assert_eq!(first.item_node_id, "PVTI_1");
    assert_eq!(
        first
            .project_fields
            .get("GitHub Project Item REST ID")
            .and_then(serde_json::Value::as_u64),
        Some(190539790)
    );
    assert_eq!(
        first
            .project_fields
            .get("Status")
            .and_then(serde_json::Value::as_str),
        Some("Todo")
    );
    assert_eq!(
        first
            .project_fields
            .get("Main Agent")
            .and_then(serde_json::Value::as_str),
        Some("v=1 lane=main")
    );
    assert_eq!(
        first
            .project_fields
            .get("Parent Integration Branch")
            .and_then(serde_json::Value::as_str),
        Some("integration/issue-468-feedback-intake")
    );

    let mut issue = issue("Todo");
    issue.id = "I_1".into();
    apply_rest_project_item_overlays(std::slice::from_mut(&mut issue), &overlays);
    assert_eq!(issue.item_id.as_deref(), Some("PVTI_1"));
    assert_eq!(
        issue
            .project_fields
            .get("GitHub Project Item REST ID")
            .and_then(serde_json::Value::as_u64),
        Some(190539790)
    );

    let second = overlays.get("I_2").unwrap();
    assert_eq!(
        second.project_fields.get("Priority"),
        Some(&serde_json::json!(3))
    );
    assert_eq!(
        second
            .project_fields
            .get("Target Date")
            .and_then(serde_json::Value::as_str),
        Some("2026-05-22")
    );
    assert_eq!(
        second
            .project_fields
            .get("Rich Target Date")
            .and_then(serde_json::Value::as_str),
        Some("2026-07-19")
    );
}

#[test]
fn rest_project_item_overlay_keeps_graphql_values_for_null_or_unrecognized_rest_fields() {
    let response = serde_json::json!([
        {
            "id": 190539792,
            "node_id": "PVTI_3",
            "content": {"node_id": "I_3"},
            "fields": [
                {
                    "id": 1,
                    "name": "Parent Integration Branch",
                    "data_type": "text",
                    "value": null
                },
                {
                    "id": 2,
                    "name": "Target Date",
                    "data_type": "date",
                    "value": {"raw": ["not", "text"]}
                },
                {
                    "id": 3,
                    "name": "Release Notes",
                    "data_type": "text"
                }
            ]
        }
    ]);
    let overlays = rest_project_item_overlays_from_response(&response).unwrap();
    let mut issue = issue("Todo");
    issue.id = "I_3".into();
    issue.project_fields.insert(
        "Parent Integration Branch".into(),
        serde_json::json!("integration/issue-468-feedback-intake"),
    );
    issue
        .project_fields
        .insert("Target Date".into(), serde_json::json!("2026-07-19"));
    issue.project_fields.insert(
        "Release Notes".into(),
        serde_json::json!("GraphQL queue-scan evidence"),
    );

    apply_rest_project_item_overlays(std::slice::from_mut(&mut issue), &overlays);

    assert_eq!(
        issue
            .project_fields
            .get("Parent Integration Branch")
            .and_then(serde_json::Value::as_str),
        Some("integration/issue-468-feedback-intake")
    );
    assert_eq!(
        issue
            .project_fields
            .get("Target Date")
            .and_then(serde_json::Value::as_str),
        Some("2026-07-19")
    );
    assert_eq!(
        issue
            .project_fields
            .get("Release Notes")
            .and_then(serde_json::Value::as_str),
        Some("GraphQL queue-scan evidence")
    );
}

#[test]
fn controlled_queuescan_readback_preserves_semantic_parent_branch_for_doctor() {
    let response = serde_json::json!([
        {
            "id": 190539793,
            "node_id": "PVTI_468",
            "content": {"node_id": "I_468"},
            "fields": [
                {
                    "id": 1,
                    "name": "Parent Integration Branch",
                    "data_type": "text",
                    "value": {
                        "raw": "integration/issue-468-feedback-intake",
                        "html": "<code>integration/incorrect-html-fallback</code>"
                    }
                }
            ]
        }
    ]);
    let overlays = rest_project_item_overlays_from_response(&response).unwrap();
    let mut parent = issue("Backlog");
    parent.id = "I_468".into();
    parent.identifier = "#468".into();
    parent.project_fields.insert(
        "GitHub Native Subissues".into(),
        serde_json::json!([{"identifier": "#479"}]),
    );
    let mut subissue = issue("Todo");
    subissue.identifier = "#479".into();
    subissue.project_fields.insert(
        "GitHub Native Parent".into(),
        serde_json::json!({"identifier": "#468"}),
    );
    let mut issues = vec![parent, subissue];

    apply_rest_project_item_overlays(&mut issues, &overlays);

    assert_eq!(
        issues[0]
            .project_fields
            .get("Parent Integration Branch")
            .and_then(serde_json::Value::as_str),
        Some("integration/issue-468-feedback-intake")
    );
    let report = crate::doctor::audit_project_issues(&issues);
    assert!(!report.violations.iter().any(|violation| {
        violation.issue_ref == "#468"
            && violation.code == "parent_topology_missing_integration_branch"
    }));
}

#[test]
fn builds_rest_project_item_field_update_payloads() {
    assert_eq!(
        rest_project_item_field_update_body(
            345980099,
            ProjectFieldUpdateValue::String("f75ad846".into())
        )
        .unwrap(),
        serde_json::json!({
            "fields": [
                {"id": 345980099, "value": "f75ad846"}
            ]
        })
    );
    assert_eq!(
        rest_project_item_field_update_body(
            347408996,
            ProjectFieldUpdateValue::String("v=1 lane=main".into())
        )
        .unwrap(),
        serde_json::json!({
            "fields": [
                {"id": 347408996, "value": "v=1 lane=main"}
            ]
        })
    );
    assert_eq!(
        rest_project_item_field_update_body(42, ProjectFieldUpdateValue::Null).unwrap(),
        serde_json::json!({
            "fields": [
                {"id": 42, "value": null}
            ]
        })
    );
}

#[test]
fn rest_update_reports_graphql_fallback_reasons_for_missing_rest_ids() {
    let client = GithubProjectV2GhClient::new(&github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 9\n---\nPrompt",
        ));
    let metadata = test_metadata(vec![
        test_status_field(),
        test_text_field("Main Agent", "FIELD_MAIN_AGENT", None),
    ]);
    let field_without_rest_id = metadata.field("Main Agent").unwrap().clone();
    let mut issue_with_rest_item = issue("Todo");
    issue_with_rest_item.project_fields.insert(
        "GitHub Project Item REST ID".into(),
        serde_json::json!(190539790),
    );

    let error = client
        .update_project_item_field_rest(
            &issue_with_rest_item,
            &metadata,
            &field_without_rest_id,
            ProjectFieldUpdateValue::String("v=1 lane=main".into()),
        )
        .unwrap_err();
    assert!(format!("{error}").contains("lacks a REST field id"));
    assert!(format!("{error}").contains("using GraphQL where available"));

    let field_with_rest_id = test_text_field("Main Agent", "FIELD_MAIN_AGENT", Some(347408996));
    let issue_without_rest_item = issue("Todo");
    let error = client
        .update_project_item_field_rest(
            &issue_without_rest_item,
            &metadata,
            &field_with_rest_id,
            ProjectFieldUpdateValue::String("v=1 lane=main".into()),
        )
        .unwrap_err();
    assert!(format!("{error}").contains("current Project read lacks REST item id"));
}

#[test]
fn resolves_project_status_option_id() {
    let metadata = ProjectMetadata {
        owner_type: ProjectV2OwnerType::User,
        project_id: "PVT_1".into(),
        status_field_id: "FIELD_STATUS".into(),
        status_options: vec![
            ("OPT_TODO".into(), "Todo".into()),
            ("OPT_DONE".into(), "Done".into()),
        ],
        fields: vec![ProjectFieldMetadata {
            id: "FIELD_STATUS".into(),
            name: "Status".into(),
            kind: ProjectFieldKind::SingleSelect,
            options: vec![
                ("OPT_TODO".into(), "Todo".into()),
                ("OPT_DONE".into(), "Done".into()),
            ],
            rest_id: None,
        }],
    };

    assert_eq!(
        status_option_id(&metadata, "Todo", "Status").unwrap(),
        "OPT_TODO"
    );
    assert!(status_option_id(&metadata, "Agent Review", "Status").is_err());
}

#[test]
fn parses_added_project_item_id() {
    let response = serde_json::json!({
        "data": {
            "addProjectV2ItemById": {
                "item": {
                    "id": "PVTI_35"
                }
            }
        }
    });

    assert_eq!(
        project_item_id_from_add_response(&response).unwrap(),
        "PVTI_35"
    );
    assert!(project_item_id_from_add_response(&serde_json::json!({"data": {}})).is_err());
}

#[test]
fn claim_decision_identifies_claimable_active_and_external_states() {
    let config = github_config(
        r#"---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: shea-symphony
  project_owner: Alive24
  project_number: 9
---
Prompt
"#,
    );

    assert_eq!(
        claim_decision(&issue("Todo"), &config),
        ClaimDecision::Claimable
    );
    assert_eq!(
        claim_decision(&issue("Rework"), &config),
        ClaimDecision::Claimable
    );
    assert_eq!(
        claim_decision(&issue("In Progress"), &config),
        ClaimDecision::AlreadyInProgress
    );
    assert_eq!(
        claim_decision(&issue("Agent Review"), &config),
        ClaimDecision::StopAndReplan {
            current_state: "Agent Review".into()
        }
    );
}

#[test]
fn status_update_required_skips_same_mapped_state() {
    assert!(!status_update_required(
        &issue("In Progress"),
        " in progress "
    ));
    assert!(!status_update_required(
        &issue("Agent Review"),
        "Agent Review"
    ));
    assert!(status_update_required(&issue("Todo"), "In Progress"));
}

#[test]
fn parses_linear_issue_payload_into_normalized_tracker_issue() {
    let response = serde_json::json!({
        "data": {
            "issues": {
                "nodes": [
                    {
                        "id": "LIN_1",
                        "identifier": "JAD-10",
                        "title": "Implement Linear adapter",
                        "description": "body",
                        "priority": 2,
                        "state": {"name": "Todo"},
                        "branchName": "feature/jad-10",
                        "url": "https://linear.app/acme/issue/JAD-10/implement-linear-adapter",
                        "assignee": {"id": "USR_1"},
                        "labels": {"nodes": [{"name": "Dogfood"}]},
                        "inverseRelations": {
                            "nodes": [
                                {
                                    "type": "blocks",
                                    "issue": {
                                        "id": "LIN_BLOCKER",
                                        "identifier": "JAD-9",
                                        "state": {"name": "In Progress"}
                                    }
                                },
                                {
                                    "type": "related",
                                    "issue": {
                                        "id": "LIN_RELATED",
                                        "identifier": "JAD-8",
                                        "state": {"name": "Done"}
                                    }
                                }
                            ]
                        },
                        "createdAt": "2026-05-10T00:00:00Z",
                        "updatedAt": "2026-05-10T01:00:00Z"
                    }
                ],
                "pageInfo": {
                    "hasNextPage": false,
                    "endCursor": null
                }
            }
        }
    });

    let (issues, next_cursor, has_next) = linear_issues_from_response(&response).unwrap();

    assert!(!has_next);
    assert_eq!(next_cursor, None);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].tracker_kind, "linear");
    assert_eq!(issues[0].id, "LIN_1");
    assert_eq!(issues[0].identifier, "JAD-10");
    assert_eq!(issues[0].state, "Todo");
    assert_eq!(issues[0].assignees, vec!["USR_1"]);
    assert_eq!(issues[0].labels, vec!["dogfood"]);
    assert_eq!(issues[0].priority, Some(2));
    assert_eq!(issues[0].branch_name.as_deref(), Some("feature/jad-10"));
    assert_eq!(issues[0].blocked_by.len(), 1);
    assert_eq!(issues[0].blocked_by[0].identifier.as_deref(), Some("JAD-9"));
}

#[test]
fn maps_normalized_state_to_linear_state_name() {
    let config = github_config(
        r#"---
tracker:
  kind: linear
  project_slug: shea-symphony
  fixture_path: issues.json
  state_map:
    in_progress: Started
    agent_review: Verification Queue
---
Prompt
"#,
    );

    assert_eq!(
        linear_state_option_name(&config, "in_progress").unwrap(),
        "Started"
    );
    assert_eq!(
        linear_state_option_name(&config, " verification queue ").unwrap(),
        "Verification Queue"
    );
    assert!(linear_state_option_name(&config, "unknown").is_err());
}

#[test]
fn detects_linear_graphql_error_payloads() {
    let response = serde_json::json!({
        "errors": [
            {"message": "Linear API rejected the query"}
        ]
    });

    assert_eq!(
        linear_graphql_error_message(&response).as_deref(),
        Some("Linear GraphQL returned errors: Linear API rejected the query")
    );
    assert!(linear_graphql_error_message(&serde_json::json!({"data": {}})).is_none());
}

#[test]
fn duplicate_workpad_body_removes_marker_text() {
    let marker = "<!-- shea-symphony-workpad -->";
    let body = duplicate_workpad_body(marker);

    assert!(!body.contains(marker));
}

#[test]
fn follow_up_body_preserves_related_context() {
    let body = follow_up_issue_body(&FollowUpIssueInput {
        title: "Follow-up".into(),
        body: "Main body".into(),
        assignees: Vec::new(),
        project_id: Some("PVT_1".into()),
        related_issue_ref: Some("#3".into()),
        blocked_by_issue_ref: Some("#2".into()),
    });

    assert!(body.contains("Main body"));
    assert!(body.contains("Related issue: #3"));
    assert!(body.contains("Blocked by: #2"));
    assert!(body.contains("Project id: PVT_1"));
}

#[test]
fn detects_graphql_error_payloads() {
    let response = serde_json::json!({
        "errors": [
            {"message": "Could not resolve to a ProjectV2"}
        ]
    });

    assert_eq!(
        graphql_error_message(&response).as_deref(),
        Some("GitHub GraphQL returned errors: Could not resolve to a ProjectV2")
    );
    assert!(graphql_error_message(&serde_json::json!({"data": {}})).is_none());
}

#[test]
fn targeted_github_issue_response_parses_project_item_status() {
    let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 9\n---\nPrompt",
        );
    let response = serde_json::json!({
        "data": {
            "repository": {
                "issue": {
                    "__typename": "Issue",
                    "id": "I_349",
                    "number": 349,
                    "title": "Add ProjectV2 metadata cache",
                    "body": "Issue body",
                    "url": "https://github.com/Alive24/shea-symphony/issues/349",
                    "state": "OPEN",
                    "createdAt": "2026-05-21T00:00:00Z",
                    "updatedAt": "2026-05-21T00:10:00Z",
                    "labels": { "nodes": [{ "name": "area:tracker" }] },
                    "assignees": { "nodes": [{ "login": "Alive24" }] },
                    "parent": null,
                    "subIssues": { "nodes": [] },
                    "closedByPullRequestsReferences": { "nodes": [] },
                    "comments": { "nodes": [] },
                    "recentComments": {
                        "nodes": [
                            {
                                "body": "Shea Symphony linked pull request: https://github.com/Alive24/shea-symphony/pull/355"
                            }
                        ]
                    },
                    "projectItems": {
                        "nodes": [
                            {
                                "id": "PVTI_OTHER",
                                "project": { "number": 8 },
                                "fieldValues": { "nodes": [] }
                            },
                            {
                                "id": "PVTI_349",
                                "project": { "number": 9 },
                                "fieldValues": {
                                    "nodes": [
                                        {
                                            "name": "In Progress",
                                            "field": { "name": "Status" }
                                        },
                                        {
                                            "text": "v=1 issue=#349 lane=main",
                                            "field": { "name": "Main Agent" }
                                        }
                                    ]
                                }
                            }
                        ]
                    }
                }
            }
        }
    });

    let issue = issue_from_repository_issue_response(&response, &config)
        .unwrap()
        .unwrap();

    assert_eq!(issue.identifier, "#349");
    assert_eq!(issue.item_id.as_deref(), Some("PVTI_349"));
    assert_eq!(issue.state, "In Progress");
    assert_eq!(issue.assignees, vec!["Alive24"]);
    assert_eq!(
        issue.project_fields.get("Main Agent"),
        Some(&serde_json::Value::String(
            "v=1 issue=#349 lane=main".into()
        ))
    );
    assert!(issue
        .linked_pull_requests
        .iter()
        .any(|pr| pr.number == Some(355)));
}

#[test]
fn classifies_project_state_failures() {
    assert_eq!(
        classify_project_state_failure_message("API rate limit exceeded"),
        ProjectStateFailureKind::RateLimit
    );
    assert_eq!(
        classify_project_state_failure_message("GraphQL resource limit exceeded"),
        ProjectStateFailureKind::ResourceLimit
    );
    assert_eq!(
        classify_project_state_failure_message("could not resolve host api.github.com"),
        ProjectStateFailureKind::Network
    );
    assert_eq!(
        classify_project_state_failure_message("error connecting to api.github.com"),
        ProjectStateFailureKind::Network
    );
    let graphql_eof = r#"GitHub GraphQL operation failed kind=unknown: Post "https://api.github.com/graphql": EOF"#;
    assert_eq!(
        classify_project_state_failure_message(graphql_eof),
        ProjectStateFailureKind::Network
    );
    assert!(project_state_error_is_retryable(
        &TrackerError::IntegrationUnavailable(graphql_eof.into())
    ));
    for status in [
        "HTTP 502 Bad Gateway",
        "HTTP 503 Service Unavailable",
        "HTTP 504 Gateway Timeout",
    ] {
        assert_eq!(
            classify_project_state_failure_message(status),
            ProjectStateFailureKind::TransientBackend
        );
        assert!(project_state_error_is_retryable(
            &TrackerError::IntegrationUnavailable(status.into())
        ));
    }
    assert_eq!(
        classify_project_state_failure_message("HTTP 403 Resource not accessible by integration"),
        ProjectStateFailureKind::Auth
    );
    assert_eq!(
        classify_project_state_failure_message(
            "GitHub GraphQL returned errors: Could not resolve to a ProjectV2"
        ),
        ProjectStateFailureKind::Schema
    );
    assert_eq!(
        classify_project_state_failure_message("GitHub GraphQL operation timed out after 30000ms"),
        ProjectStateFailureKind::Network
    );
    assert_eq!(
        classify_project_state_failure_message(
            "GitHub GraphQL operation failed: HTTP 502 Bad Gateway"
        ),
        ProjectStateFailureKind::TransientBackend
    );
    assert_eq!(
        classify_project_state_failure_message(
            "GitHub GraphQL operation failed: HTTP 500 Internal Server Error"
        ),
        ProjectStateFailureKind::TransientBackend
    );
    assert_eq!(
        classify_project_state_failure_message(
            "GitHub GraphQL returned errors: Field 'foo' doesn't exist on type ProjectV2"
        ),
        ProjectStateFailureKind::Schema
    );
    assert_eq!(
        classify_project_state_error(&TrackerError::Payload(
            "partial ProjectV2 response missing status field \"Status\" for issue #7".into()
        )),
        ProjectStateFailureKind::PartialResponse
    );
    assert_eq!(
        classify_project_state_failure_message(
            "invalid GitHub GraphQL JSON: EOF while parsing a value at line 1 column 0"
        ),
        ProjectStateFailureKind::Payload
    );
    assert_eq!(
        classify_project_state_error(&TrackerError::NotImplemented(
            "missing CLI capability for raw issue content reads".into()
        )),
        ProjectStateFailureKind::MissingCapability
    );
}

#[cfg(unix)]
#[test]
fn command_timeout_returns_transient_tracker_error() {
    let args = vec!["-c".into(), "sleep 0.05".into()];
    let error = run_command_with_timeout(
        "sh",
        &args,
        "GitHub GraphQL operation",
        Duration::from_millis(1),
    )
    .unwrap_err();

    assert_eq!(
        classify_project_state_error(&error),
        ProjectStateFailureKind::Network
    );
    assert!(error.to_string().contains("timed out after"));
}

#[test]
fn project_issue_missing_status_is_partial_response_error() {
    let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );
    let response = serde_json::json!({
        "data": {
            "organization": {
                "projectV2": {
                    "items": {
                        "nodes": [
                            {
                                "id": "PVTI_1",
                                "content": {
                                    "__typename": "Issue",
                                    "id": "I_1",
                                    "number": 7,
                                    "title": "Project read hardening"
                                },
                                "fieldValues": {
                                    "nodes": []
                                }
                            }
                        ],
                        "pageInfo": {
                            "hasNextPage": false,
                            "endCursor": null
                        }
                    }
                }
            }
        }
    });

    let error = issues_from_project_response(&response, &config).unwrap_err();

    assert_eq!(
        classify_project_state_error(&error),
        ProjectStateFailureKind::PartialResponse
    );
    assert!(error.to_string().contains("missing status field"));
}
