use serde_json::Value;

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
        .map(|topology| topology["name"].as_str().expect("unsafe example should have a name"))
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
            topology["violates"].as_str().is_some_and(|value| !value.is_empty()),
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
        "Issue #274 should teach lane flows how to use the parent integration branch",
    ] {
        assert!(
            doc.contains(required),
            "topology doc should contain required rule: {required}"
        );
    }
}
