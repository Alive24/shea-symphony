use crate::model::{LinkedPullRequest, SessionStatusSnapshot, TrackerIssue};
use std::collections::BTreeMap;

pub(super) fn issue(identifier: &str, state: &str) -> TrackerIssue {
    TrackerIssue {
        tracker_kind: "github_project_v2".into(),
        id: identifier.into(),
        item_id: None,
        identifier: identifier.into(),
        title: format!("Issue {identifier}"),
        description: None,
        url: None,
        state: state.into(),
        labels: Vec::new(),
        assignees: Vec::new(),
        priority: None,
        branch_name: None,
        linked_pull_requests: Vec::new(),
        blocked_by: Vec::new(),
        project_fields: BTreeMap::new(),
        created_at: None,
        updated_at: None,
    }
}

pub(super) fn linked_pr(url: &str, state: &str) -> LinkedPullRequest {
    LinkedPullRequest {
        id: Some("PR_1".into()),
        number: Some(1),
        url: Some(url.into()),
        state: Some(state.into()),
        ..Default::default()
    }
}

pub(super) fn linked_pr_to(url: &str, state: &str, base: &str) -> LinkedPullRequest {
    let mut pr = linked_pr(url, state);
    pr.base_ref_name = Some(base.into());
    pr
}

pub(super) fn with_native_parent(mut issue: TrackerIssue, parent: &str) -> TrackerIssue {
    issue.project_fields.insert(
        "GitHub Native Parent".into(),
        serde_json::json!({ "identifier": parent }),
    );
    issue
}

pub(super) fn with_native_subissues(mut issue: TrackerIssue, subissues: &[&str]) -> TrackerIssue {
    issue.project_fields.insert(
        "GitHub Native Subissues".into(),
        serde_json::Value::Array(
            subissues
                .iter()
                .map(|issue_ref| serde_json::json!({ "identifier": issue_ref }))
                .collect(),
        ),
    );
    issue
}

pub(super) fn with_parent_branch(mut issue: TrackerIssue, branch: &str) -> TrackerIssue {
    issue.description = Some(format!(
        "## Parent Topology\n\nParent integration branch: `{branch}`"
    ));
    issue
}

pub(super) fn session(identifier: Option<&str>, status: &str) -> SessionStatusSnapshot {
    SessionStatusSnapshot {
        session_id: "jade-main-202-attempt-1-runtime".into(),
        lane: "main".into(),
        backend: "tmux".into(),
        run_id: None,
        status: status.into(),
        evidence_source: "registry".into(),
        evidence: "registry record has not updated for 19000ms".into(),
        issue_identifier: identifier.map(str::to_string),
        issue_title: Some("Runtime session".into()),
        attach_command: Some("tmux attach-session -t jade-main-202-attempt-1-runtime".into()),
        log_path: Some("/tmp/jade/logs/tmux/jade-main-202-attempt-1-runtime.log".into()),
        updated_at_ms: 1_000,
    }
}

pub(super) fn with_github_issue_state(mut issue: TrackerIssue, state: &str) -> TrackerIssue {
    issue.project_fields.insert(
        "GitHub Issue State".into(),
        serde_json::Value::String(state.into()),
    );
    issue
}
