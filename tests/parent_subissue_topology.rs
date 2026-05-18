use serde_json::Value;

use jade_symphony::handoff::{
    expected_merge_base_branch_for_issue, plan_issue_handoff, BranchTargetRole,
};
use jade_symphony::model::TrackerIssue;

fn fixture() -> Value {
    let raw = include_str!("../examples/fixtures/parent-subissue-topology.json");
    serde_json::from_str(raw).expect("parent/subissue topology fixture should be valid JSON")
}

fn doc() -> &'static str {
    include_str!("../docs/parent-subissue-topology.md")
}

#[test]
fn happy_path_fixture_matches_documented_topology() {
    let fixture = fixture();
    let happy_path = &fixture["happy_path"];
    let parent_branch = happy_path["parent_integration_branch"]
        .as_str()
        .expect("happy path should name a parent integration branch");

    assert_eq!(fixture["source_of_truth"], "github_native_subissues");
    assert_eq!(happy_path["parent_issue"], "#243");
    assert_eq!(
        parent_branch,
        "integration/issue-243-parent-subissue-orchestration"
    );
    assert!(
        parent_branch.starts_with("integration/issue-243-"),
        "parent integration branch should include the parent issue number"
    );

    let subissues = happy_path["subissues"]
        .as_array()
        .expect("happy path should include subissues");
    assert_eq!(subissues.len(), 3);
    for subissue in subissues {
        assert_eq!(subissue["native_parent"], "#243");
        assert_eq!(subissue["pr_base"], parent_branch);

        let done_allowed_when = subissue["done_allowed_when"]
            .as_array()
            .expect("subissue should list Done conditions");
        assert!(
            done_allowed_when
                .iter()
                .any(|condition| condition == "pr_merged_into_parent_branch"),
            "subissue Done must require merge into the parent integration branch"
        );
    }

    assert_eq!(happy_path["parent_final_pr"]["head"], parent_branch);
    assert_eq!(happy_path["parent_final_pr"]["base"], "main");

    let parent_gate = happy_path["parent_human_review_gate"]
        .as_array()
        .expect("parent Human Review gate should be listed");
    for required in [
        "all_native_subissues_done",
        "all_subissue_prs_merged_into_parent_branch",
        "parent_final_pr_targets_main",
        "independent_parent_review_passed",
    ] {
        assert!(
            parent_gate.iter().any(|condition| condition == required),
            "parent gate should include {required}"
        );
    }
}

#[test]
fn fixture_covers_unsafe_topologies_for_later_doctor_checks() {
    let fixture = fixture();
    let unsafe_topologies = fixture["unsafe_topologies"]
        .as_array()
        .expect("fixture should include unsafe topology examples");
    let names: Vec<_> = unsafe_topologies
        .iter()
        .map(|topology| {
            topology["name"]
                .as_str()
                .expect("unsafe example should have a name")
        })
        .collect();

    for required in [
        "subissue-pr-targets-main",
        "body-only-hierarchy",
        "subissue-done-before-parent-merge",
        "parent-review-before-subissues-done",
    ] {
        assert!(
            names.contains(&required),
            "fixture should include unsafe example {required}"
        );
    }

    for topology in unsafe_topologies {
        assert!(
            topology["violates"]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "unsafe examples should explain the violated rule"
        );
        assert!(
            topology["recommended_doctor_invariant"]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "unsafe examples should name the later doctor invariant"
        );
    }
}

#[test]
fn documentation_states_the_non_competing_sources_and_boundaries() {
    let doc = doc();

    for required in [
        "GitHub native sub-issue links are the source of truth",
        "Subissue PRs target the parent integration branch by default",
        "The parent issue remains the final Human Review unit",
        "The Main Agent still stops at `Agent Review`",
        "Issue #273 should turn these into doctor invariants",
        "Issue #274 should teach",
        "the parent integration branch during live execution",
    ] {
        assert!(
            doc.contains(required),
            "topology doc should contain required rule: {required}"
        );
    }
}

#[test]
fn lane_handoff_uses_fixture_parent_branch_for_subissue_and_parent() {
    let fixture = fixture();
    let happy_path = &fixture["happy_path"];
    let parent_branch = happy_path["parent_integration_branch"].as_str().unwrap();

    let subissue = tracker_issue(
        "#274",
        "Teach lane flows about parent integration branches",
        Some("#243"),
        None,
        Some(parent_branch),
    );
    let subissue_plan = plan_issue_handoff(
        std::path::Path::new("/tmp/jade-workspaces"),
        &subissue,
        "main",
    )
    .unwrap();

    assert_eq!(subissue_plan.branch_target.role, BranchTargetRole::Subissue);
    assert_eq!(subissue_plan.pull_request.base_branch, parent_branch);
    assert_eq!(
        expected_merge_base_branch_for_issue(&subissue, "main"),
        parent_branch
    );

    let parent = tracker_issue(
        "#243",
        "Complete parent/subissue orchestration umbrella gating",
        None,
        Some("#272, #273, #274"),
        Some(parent_branch),
    );
    let parent_plan = plan_issue_handoff(
        std::path::Path::new("/tmp/jade-workspaces"),
        &parent,
        "main",
    )
    .unwrap();

    assert_eq!(
        parent_plan.branch_target.role,
        BranchTargetRole::ParentIssue
    );
    assert_eq!(parent_plan.branch_name, parent_branch);
    assert_eq!(parent_plan.pull_request.head_branch, parent_branch);
    assert_eq!(parent_plan.pull_request.base_branch, "main");
}

fn tracker_issue(
    identifier: &str,
    title: &str,
    native_parent: Option<&str>,
    native_subissues: Option<&str>,
    parent_integration_branch: Option<&str>,
) -> TrackerIssue {
    let mut issue = TrackerIssue {
        tracker_kind: "fixture".into(),
        id: identifier.into(),
        item_id: None,
        identifier: identifier.into(),
        title: title.into(),
        description: None,
        url: None,
        state: "Merging".into(),
        labels: Vec::new(),
        assignees: Vec::new(),
        priority: None,
        branch_name: None,
        linked_pull_requests: Vec::new(),
        blocked_by: Vec::new(),
        project_fields: Default::default(),
        created_at: None,
        updated_at: None,
    };

    if let Some(native_parent) = native_parent {
        issue.project_fields.insert(
            "Native Parent Issue".into(),
            serde_json::json!(native_parent),
        );
    }
    if let Some(native_subissues) = native_subissues {
        issue.project_fields.insert(
            "Native Subissues".into(),
            serde_json::json!(native_subissues),
        );
    }
    if let Some(parent_integration_branch) = parent_integration_branch {
        issue.project_fields.insert(
            "Parent Integration Branch".into(),
            serde_json::json!(parent_integration_branch),
        );
    }
    issue
}
