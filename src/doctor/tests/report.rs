use super::super::{
    audit_project_issues, human_review_repair_candidates, render_human_review_repair_workpad,
    render_project_audit_report, render_project_audit_report_json, ProjectAuditReport,
};
use super::issue;

#[test]
fn human_review_repair_candidates_are_specific_to_missing_review_evidence() {
    let report =
        audit_project_issues(&[issue("#41", "Human Review"), issue("#57", "Agent Review")]);

    let candidates = human_review_repair_candidates(&report);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].issue_ref, "#41");
}

#[test]
fn human_review_repair_workpad_preserves_authority_boundary() {
    let report = audit_project_issues(&[issue("#41", "Human Review")]);
    let workpad = render_human_review_repair_workpad(&report.violations[0]);

    assert!(workpad.contains("## Jade Symphony Doctor Triage"));
    assert!(workpad.contains("moving this issue back to `Agent Review`"));
    assert!(workpad.contains("does not set `Human Review`"));
}

#[test]
fn renders_operator_summary() {
    let report = audit_project_issues(&[issue("#57", "Agent Review")]);
    let rendered = render_project_audit_report(&report);

    assert!(rendered.contains("project_doctor=ok"));
    assert!(rendered.contains("violations=1"));
    assert!(rendered.contains("agent_review_missing_pr_handoff"));
}

#[test]
fn renders_json_report() {
    let mut report = audit_project_issues(&[issue("#57", "Agent Review")]);
    report.integration_gaps.push("missing scope".into());

    let rendered = render_project_audit_report_json(&report).unwrap();
    let parsed: ProjectAuditReport = serde_json::from_str(&rendered).unwrap();

    assert_eq!(parsed.total_issues, 1);
    assert_eq!(parsed.blocker_count(), 1);
    assert_eq!(parsed.violations[0].code, "agent_review_missing_pr_handoff");
    assert_eq!(parsed.integration_gaps, vec!["missing scope"]);
    assert!(!rendered.contains("\nintegration_gap="));
}
