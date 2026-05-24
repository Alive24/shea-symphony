use crate::runtime_state::{RuntimeIssueState, RuntimeState};

use super::super::{audit_project_issues_with_context, ProjectDoctorContext};
use super::support::{issue, linked_pr, session, with_github_issue_state};

#[test]
fn accepts_completed_main_session_for_agent_review_issue() {
    let mut issue = issue("#57", "Agent Review");
    issue.linked_pull_requests.push(linked_pr(
        "https://github.com/Alive24/shea-symphony/pull/57",
        "OPEN",
    ));
    let context = ProjectDoctorContext {
        runtime_state: None,
        runtime_states: Vec::new(),
        sessions: vec![session(Some("#57"), "completed")],
        now_ms: 20_000,
        stale_after_ms: 10_000,
    };

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(report.is_clean());
}

#[test]
fn reports_raw_unknown_session_status_drift() {
    let mut drifted = session(Some("#202"), "unknown");
    drifted.evidence = "unknown persisted session status recorded_legacy".into();
    let context = ProjectDoctorContext {
        runtime_state: None,
        runtime_states: Vec::new(),
        sessions: vec![drifted],
        now_ms: 20_000,
        stale_after_ms: 10_000,
    };
    let report = audit_project_issues_with_context(&[issue("#202", "In Progress")], Some(&context));

    let violation = report
        .violations
        .iter()
        .find(|violation| violation.code == "tmux_session_needs_operator_attention")
        .unwrap();
    assert!(violation
        .message
        .contains("unknown persisted session status recorded_legacy"));
}

#[test]
fn skips_done_issue_runtime_and_session_checks() {
    let issue = with_github_issue_state(issue("#202", "Done"), "CLOSED");
    let mut context = runtime_context("#202", 1_000);
    context.runtime_state.as_mut().unwrap().backend_session_id = Some("missing-session".into());
    context.sessions = vec![session(Some("#202"), "stale")];

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(report.is_clean());
}

#[test]
fn reports_stale_runtime_state_for_in_progress_issue() {
    let issue = issue("#202", "In Progress");
    let context = runtime_context("#202", 1_000);

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(report
        .violations
        .iter()
        .any(|violation| violation.code == "runtime_state_stale"));
}

#[test]
fn reports_runtime_state_tracker_mismatch() {
    let issue = issue("#202", "Agent Review");
    let context = runtime_context("#202", 19_000);

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(report
        .violations
        .iter()
        .any(|violation| violation.code == "runtime_state_tracker_mismatch"));
}

#[test]
fn reports_unsafe_runtime_active_issue_disagreement() {
    let issue = issue("#203", "In Progress");
    let context = runtime_context("#202", 19_000);

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(report
        .violations
        .iter()
        .any(|violation| violation.code == "runtime_active_issue_disagrees"));
}

#[test]
fn accepts_multiple_in_progress_issues_with_matching_runtime_states() {
    let context = ProjectDoctorContext {
        runtime_state: None,
        runtime_states: vec![runtime_state("#202", 19_000), runtime_state("#203", 19_000)],
        sessions: Vec::new(),
        now_ms: 20_000,
        stale_after_ms: 10_000,
    };

    let report = audit_project_issues_with_context(
        &[issue("#202", "In Progress"), issue("#203", "In Progress")],
        Some(&context),
    );

    assert!(!report
        .violations
        .iter()
        .any(|violation| violation.code == "runtime_active_issue_disagrees"));
}

#[test]
fn reports_in_progress_issue_missing_matching_runtime_state() {
    let context = ProjectDoctorContext {
        runtime_state: None,
        runtime_states: vec![runtime_state("#202", 19_000), runtime_state("#203", 19_000)],
        sessions: Vec::new(),
        now_ms: 20_000,
        stale_after_ms: 10_000,
    };

    let report = audit_project_issues_with_context(&[issue("#204", "In Progress")], Some(&context));

    assert!(report
        .violations
        .iter()
        .any(|violation| violation.code == "runtime_active_issue_disagrees"));
}

#[test]
fn reports_stale_tmux_session_for_matching_issue() {
    let issue = issue("#202", "In Progress");
    let mut context = runtime_context("#202", 19_000);
    context.sessions = vec![session(Some("#202"), "stale")];

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(report
        .violations
        .iter()
        .any(|violation| violation.code == "tmux_session_stale"));
}

#[test]
fn reports_app_server_session_attention_without_tmux_recovery_wording() {
    let issue = issue("#202", "In Progress");
    let mut context = runtime_context("#202", 19_000);
    let mut app_server_session = session(Some("#202"), "failed");
    app_server_session.backend = "codex".into();
    app_server_session.attach_command = None;
    app_server_session.log_path = None;
    context.sessions = vec![app_server_session];

    let report = audit_project_issues_with_context(&[issue], Some(&context));
    let violation = report
        .violations
        .iter()
        .find(|violation| violation.code == "tmux_session_needs_operator_attention")
        .expect("expected session attention warning");

    assert!(violation
        .message
        .contains("Registered codex session `shea-main-202-attempt-1-runtime`"));
    assert!(violation.suggestion.contains("runtime artifacts"));
    assert!(!violation.suggestion.contains("attach command"));
}

#[test]
fn reports_runtime_session_missing_registry() {
    let issue = issue("#202", "In Progress");
    let mut context = runtime_context("#202", 19_000);
    context.runtime_state.as_mut().unwrap().backend_session_id = Some("missing-session".into());

    let report = audit_project_issues_with_context(&[issue], Some(&context));

    assert!(report
        .violations
        .iter()
        .any(|violation| violation.code == "runtime_session_missing_registry"));
}

#[test]
fn reports_unattributed_tmux_session() {
    let context = ProjectDoctorContext {
        runtime_state: None,
        runtime_states: Vec::new(),
        sessions: vec![session(None, "running")],
        now_ms: 20_000,
        stale_after_ms: 10_000,
    };

    let report = audit_project_issues_with_context(&[], Some(&context));

    assert_eq!(report.violations[0].code, "tmux_session_missing_issue");
}

fn runtime_state(identifier: &str, updated_at_ms: u64) -> RuntimeState {
    let mut runtime_state = RuntimeState::active(
        RuntimeIssueState {
            id: format!("ISSUE_{identifier}"),
            identifier: identifier.into(),
        },
        "dry-run",
    );
    runtime_state.updated_at_ms = Some(updated_at_ms);
    runtime_state
}

fn runtime_context(identifier: &str, updated_at_ms: u64) -> ProjectDoctorContext {
    let runtime_state = runtime_state(identifier, updated_at_ms);
    ProjectDoctorContext {
        runtime_state: Some(runtime_state),
        runtime_states: Vec::new(),
        sessions: Vec::new(),
        now_ms: 20_000,
        stale_after_ms: 10_000,
    }
}
