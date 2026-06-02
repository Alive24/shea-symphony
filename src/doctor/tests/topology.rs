use super::super::{audit_project_issues, AuditSeverity};
use super::support::{
    issue, linked_pr_to, with_native_parent, with_native_subissues, with_parent_branch,
};

#[test]
fn accepts_happy_parent_subissue_topology() {
    let parent_branch = "integration/issue-243-parent-subissue-orchestration";
    let mut parent = with_parent_branch(
        with_native_subissues(issue("#243", "Human Review"), &["#272", "#273"]),
        parent_branch,
    );
    parent.description = Some(format!(
        "Parent integration branch: `{parent_branch}`\nReview pass evidence: `recorded`"
    ));
    let mut subissue_one = with_native_parent(issue("#272", "Done"), "#243");
    subissue_one.linked_pull_requests.push(linked_pr_to(
        "https://github.com/Alive24/shea-symphony/pull/272",
        "MERGED",
        parent_branch,
    ));
    let mut subissue_two = with_native_parent(issue("#273", "Done"), "#243");
    subissue_two.linked_pull_requests.push(linked_pr_to(
        "https://github.com/Alive24/shea-symphony/pull/273",
        "MERGED",
        parent_branch,
    ));

    let report = audit_project_issues(&[parent, subissue_one, subissue_two]);

    assert!(report.is_clean());
}

#[test]
fn reports_subissue_pr_targeting_main() {
    let parent_branch = "integration/issue-243-parent-subissue-orchestration";
    let parent = with_parent_branch(
        with_native_subissues(issue("#243", "Todo"), &["#273"]),
        parent_branch,
    );
    let mut subissue = with_native_parent(issue("#273", "Agent Review"), "#243");
    subissue.linked_pull_requests.push(linked_pr_to(
        "https://github.com/Alive24/shea-symphony/pull/273",
        "OPEN",
        "main",
    ));

    let report = audit_project_issues(&[parent, subissue]);

    assert!(report.violations.iter().any(|violation| {
        violation.code == "subissue_pr_targets_main" && violation.severity == AuditSeverity::Blocker
    }));
}

#[test]
fn reports_missing_parent_integration_branch_evidence() {
    let parent = with_native_subissues(issue("#243", "Todo"), &["#273"]);
    let subissue = with_native_parent(issue("#273", "Todo"), "#243");

    let report = audit_project_issues(&[parent, subissue]);

    assert!(report.violations.iter().any(|violation| {
        violation.code == "parent_topology_missing_integration_branch"
            && violation.severity == AuditSeverity::Blocker
    }));
}

#[test]
fn reports_done_subissue_without_parent_merge_evidence() {
    let parent_branch = "integration/issue-243-parent-subissue-orchestration";
    let parent = with_parent_branch(
        with_native_subissues(issue("#243", "Todo"), &["#272"]),
        parent_branch,
    );
    let mut subissue = with_native_parent(issue("#272", "Done"), "#243");
    subissue.linked_pull_requests.push(linked_pr_to(
        "https://github.com/Alive24/shea-symphony/pull/272",
        "OPEN",
        parent_branch,
    ));

    let report = audit_project_issues(&[parent, subissue]);

    assert!(report.violations.iter().any(|violation| {
        violation.code == "subissue_done_missing_parent_merge"
            && violation.severity == AuditSeverity::Blocker
    }));
}

#[test]
fn reports_parent_human_review_before_subissues_done() {
    let parent_branch = "integration/issue-243-parent-subissue-orchestration";
    let mut parent = with_parent_branch(
        with_native_subissues(issue("#243", "Human Review"), &["#272", "#273"]),
        parent_branch,
    );
    parent.description = Some(format!(
        "Parent integration branch: `{parent_branch}`\nReview pass evidence: `recorded`"
    ));
    let mut done_subissue = with_native_parent(issue("#272", "Done"), "#243");
    done_subissue.linked_pull_requests.push(linked_pr_to(
        "https://github.com/Alive24/shea-symphony/pull/272",
        "MERGED",
        parent_branch,
    ));
    let mut active_subissue = with_native_parent(issue("#273", "Agent Review"), "#243");
    active_subissue.linked_pull_requests.push(linked_pr_to(
        "https://github.com/Alive24/shea-symphony/pull/273",
        "MERGED",
        parent_branch,
    ));

    let report = audit_project_issues(&[parent, done_subissue, active_subissue]);

    assert!(report.violations.iter().any(|violation| {
        violation.code == "parent_human_review_subissue_not_done"
            && violation.severity == AuditSeverity::Blocker
    }));
}

#[test]
fn reports_native_subissue_in_human_review_without_real_exception() {
    let parent_branch = "integration/issue-400-native-subissue-batch";
    let parent = with_parent_branch(
        with_native_subissues(issue("#400", "Agent Review"), &["#399"]),
        parent_branch,
    );
    let mut subissue = with_native_parent(issue("#399", "Human Review"), "#400");
    subissue.description = Some(
        "Related Parent Issue or Context: native subissue under #400.\nSubissue Human Review Exception: None."
            .into(),
    );

    let report = audit_project_issues(&[parent, subissue]);

    assert!(report.violations.iter().any(|violation| {
        violation.code == "subissue_human_review_without_exception"
            && violation.severity == AuditSeverity::Blocker
            && violation.issue_ref == "#399"
    }));
}

#[test]
fn accepts_native_subissue_in_human_review_with_real_exception() {
    let parent_branch = "integration/issue-400-native-subissue-batch";
    let parent = with_parent_branch(
        with_native_subissues(issue("#400", "Agent Review"), &["#399"]),
        parent_branch,
    );
    let mut subissue = with_native_parent(issue("#399", "Human Review"), "#400");
    subissue.description =
        Some("Subissue Human Review Exception: operator must inspect live credentials.".into());

    let report = audit_project_issues(&[parent, subissue]);

    assert!(!report
        .violations
        .iter()
        .any(|violation| violation.code == "subissue_human_review_without_exception"));
}

#[test]
fn reports_body_only_parent_hierarchy_as_warning() {
    let mut subissue = issue("#274", "Todo");
    subissue.description =
        Some("Related Parent Issue or Context: subissue under parent issue #243.".into());

    let report = audit_project_issues(&[subissue]);

    assert!(report.violations.iter().any(|violation| {
        violation.code == "body_only_parent_hierarchy"
            && violation.severity == AuditSeverity::Warning
    }));
}
