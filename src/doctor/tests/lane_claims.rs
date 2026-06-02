use super::super::{
    audit_project_issues, audit_project_issues_with_context, AuditSeverity, ProjectDoctorContext,
};
use super::support::{issue, session, with_github_issue_state};

#[test]
fn reports_todo_with_main_agent_claim() {
    let mut issue = issue("#202", "Todo");
    issue.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String("codex-alpha".into()),
    );

    let report = audit_project_issues(&[issue]);

    assert!(report
        .violations
        .iter()
        .any(|violation| violation.code == "todo_main_agent_claimed"));
}

#[test]
fn skips_done_issue_legacy_lane_claims() {
    let mut issue = with_github_issue_state(issue("#67", "Done"), "CLOSED");
    issue.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String("legacy-codex-worker".into()),
    );

    let report = audit_project_issues(&[issue]);

    assert!(report.is_clean());
}

#[test]
fn reports_done_issue_active_structured_claims_as_terminal_warning() {
    let mut issue = with_github_issue_state(issue("#244", "Done"), "CLOSED");
    issue.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String(
            "v=1 lane=main actor=codex source=manual issue=#244 run=20260516T0415Z-issue244-main-a7f3 state=active thread=unknown registry=run/20260516T0415Z-issue244-main-a7f3".into(),
        ),
    );
    let context = ProjectDoctorContext {
        runtime_state: None,
        runtime_states: Vec::new(),
        sessions: Vec::new(),
        now_ms: 20_000,
        stale_after_ms: 10_000,
    };

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(report.violations.iter().any(|violation| {
        violation.code == "terminal_issue_active_lane_claim"
            && violation.severity == AuditSeverity::Warning
    }));
    assert!(!report
        .violations
        .iter()
        .any(|violation| violation.code == "active_lane_claim_missing_registry"));
}

#[test]
fn reports_active_structured_claim_missing_registry_evidence() {
    let mut issue = issue("#244", "In Progress");
    issue.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String(
            "v=1 lane=main actor=codex source=manual issue=#244 run=20260516T0415Z-issue244-main-a7f3 state=active thread=unknown registry=run/20260516T0415Z-issue244-main-a7f3".into(),
        ),
    );
    let context = ProjectDoctorContext {
        runtime_state: None,
        runtime_states: Vec::new(),
        sessions: Vec::new(),
        now_ms: 20_000,
        stale_after_ms: 10_000,
    };

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(report
        .violations
        .iter()
        .any(|violation| violation.code == "active_lane_claim_missing_registry"));
}

#[test]
fn accepts_active_structured_claim_with_manual_registry_evidence() {
    let run = "20260516T0415Z-issue244-main-a7f3";
    let mut issue = issue("#244", "In Progress");
    issue.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String(format!(
            "v=1 lane=main actor=codex source=manual issue=#244 run={run} state=active thread=unknown registry=run/{run}"
        )),
    );
    let mut session = session(Some("#244"), "recorded");
    session.run_id = Some(run.into());
    session.session_id = format!("manual-main-{run}");
    session.attach_command = None;
    session.log_path = None;
    let context = ProjectDoctorContext {
        runtime_state: None,
        runtime_states: Vec::new(),
        sessions: vec![session],
        now_ms: 20_000,
        stale_after_ms: 10_000,
    };

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(!report
        .violations
        .iter()
        .any(|violation| violation.code == "active_lane_claim_missing_registry"));
}

#[test]
fn reports_terminal_structured_claim_missing_registry_as_guidance() {
    let mut issue = issue("#244", "In Progress");
    issue.project_fields.insert(
        "Main Agent".into(),
        serde_json::Value::String(
            "v=1 lane=main actor=codex source=manual issue=#244 run=20260516T0415Z-issue244-main-a7f3 state=done thread=unknown registry=run/20260516T0415Z-issue244-main-a7f3".into(),
        ),
    );
    let context = ProjectDoctorContext {
        runtime_state: None,
        runtime_states: Vec::new(),
        sessions: Vec::new(),
        now_ms: 20_000,
        stale_after_ms: 10_000,
    };

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(report.violations.iter().any(|violation| {
        violation.code == "terminal_lane_claim_missing_registry"
            && violation.severity == AuditSeverity::Warning
    }));
    assert!(!report
        .violations
        .iter()
        .any(|violation| violation.code == "active_lane_claim_missing_registry"));
}
