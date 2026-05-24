use super::super::{
    audit_project_issues, draft_pr_repair_candidates, AGENT_REVIEW_DRAFT_PR,
    HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE,
};
use super::{issue, linked_pr, with_github_issue_state};

#[test]
fn reports_agent_review_missing_pr_handoff() {
    let report = audit_project_issues(&[issue("#57", "Agent Review")]);

    assert_eq!(report.blocker_count(), 1);
    assert_eq!(report.violations[0].code, "agent_review_missing_pr_handoff");
}

#[test]
fn accepts_agent_review_with_pr_url() {
    let mut issue = issue("#57", "Agent Review");
    issue.linked_pull_requests.push(linked_pr(
        "https://github.com/Alive24/jade-symphony/pull/57",
        "OPEN",
    ));

    let report = audit_project_issues(&[issue]);

    assert!(report.is_clean());
}

#[test]
fn reports_agent_review_with_draft_pr() {
    let mut issue = issue("#57", "Agent Review");
    let mut pr = linked_pr("https://github.com/Alive24/jade-symphony/pull/57", "OPEN");
    pr.is_draft = Some(true);
    issue.linked_pull_requests.push(pr);

    let report = audit_project_issues(&[issue]);

    assert_eq!(report.blocker_count(), 1);
    assert_eq!(report.violations[0].code, AGENT_REVIEW_DRAFT_PR);
    assert_eq!(draft_pr_repair_candidates(&report).len(), 1);
}

#[test]
fn reports_human_review_without_review_pass_evidence() {
    let report = audit_project_issues(&[issue("#41", "Human Review")]);

    assert_eq!(
        report.violations[0].code,
        HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE
    );
}

#[test]
fn accepts_human_review_with_workpad_review_pass_evidence() {
    let mut issue = issue("#41", "Human Review");
    issue.description = Some(
        [
            "<!-- jade-symphony-workpad -->",
            "## Agent Review",
            "- Decision: Independent Agent Review passed with recorded evidence; issue is ready for Human Review.",
            "- Review pass evidence: `recorded`",
            "Evidence recorded. Independent Review Agent may move this issue to Human Review; the main implementation agent must not.",
        ]
        .join("\n"),
    );

    let report = audit_project_issues(&[issue]);

    assert!(report.is_clean());
}

#[test]
fn review_agent_claim_alone_does_not_satisfy_human_review_evidence() {
    let mut issue = issue("#41", "Human Review");
    issue.project_fields.insert(
        "Review Agent".into(),
        serde_json::Value::String("Gemini A".into()),
    );

    let report = audit_project_issues(&[issue]);

    assert_eq!(
        report.violations[0].code,
        HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE
    );
}

#[test]
fn failed_review_workpad_does_not_satisfy_human_review_evidence() {
    let mut issue = issue("#41", "Human Review");
    issue.description = Some(
        [
            "<!-- jade-symphony-workpad -->",
            "## Agent Review",
            "- Decision: Agent Review needs additional context; Human Review is not allowed yet.",
            "- Review did not pass; unavailable or inconclusive review must not move to Human Review.",
        ]
        .join("\n"),
    );

    let report = audit_project_issues(&[issue]);

    assert_eq!(
        report.violations[0].code,
        HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE
    );
}

#[test]
fn reports_dirty_merging_pr() {
    let mut issue = issue("#60", "Merging");
    issue.linked_pull_requests.push(linked_pr(
        "https://github.com/Alive24/jade-symphony/pull/60",
        "OPEN",
    ));
    issue.project_fields.insert(
        "pr_merge_state".into(),
        serde_json::Value::String("DIRTY".into()),
    );

    let report = audit_project_issues(&[issue]);

    assert_eq!(report.violations[0].code, "merging_pr_not_clean");
}

#[test]
fn reports_merging_missing_pr_target() {
    let report = audit_project_issues(&[issue("#60", "Merging")]);

    assert_eq!(report.blocker_count(), 1);
    assert_eq!(report.violations[0].code, "merging_missing_pr_target");
}

#[test]
fn reports_merging_ambiguous_pr_target() {
    let mut issue = issue("#60", "Merging");
    issue.linked_pull_requests.push(linked_pr(
        "https://github.com/Alive24/jade-symphony/pull/60",
        "OPEN",
    ));
    issue.linked_pull_requests.push(linked_pr(
        "https://github.com/Alive24/jade-symphony/pull/61",
        "OPEN",
    ));

    let report = audit_project_issues(&[issue]);

    assert_eq!(report.blocker_count(), 1);
    assert_eq!(report.violations[0].code, "merging_ambiguous_pr_target");
}

#[test]
fn accepts_merging_with_one_pr_target() {
    let mut issue = issue("#60", "Merging");
    issue.linked_pull_requests.push(linked_pr(
        "https://github.com/Alive24/jade-symphony/pull/60",
        "OPEN",
    ));

    let report = audit_project_issues(&[issue]);

    assert!(report.is_clean());
}

#[test]
fn reports_done_project_item_with_open_github_issue() {
    let issue = with_github_issue_state(issue("#255", "Done"), "OPEN");

    let report = audit_project_issues(&[issue]);

    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].code, "done_project_issue_still_open");
}

#[test]
fn reports_closed_github_issue_without_done_project_status() {
    let issue = with_github_issue_state(issue("#255", "Agent Review"), "CLOSED");

    let report = audit_project_issues(&[issue]);

    assert!(report
        .violations
        .iter()
        .any(|violation| violation.code == "closed_issue_not_done"));
}

#[test]
fn reports_in_progress_with_pr_evidence() {
    let mut issue = issue("#202", "In Progress");
    issue.linked_pull_requests.push(linked_pr(
        "https://github.com/Alive24/jade-symphony/pull/202",
        "OPEN",
    ));

    let report = audit_project_issues(&[issue]);

    assert!(report
        .violations
        .iter()
        .any(|violation| violation.code == "in_progress_has_pr_evidence"));
}
