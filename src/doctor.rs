use serde::{Deserialize, Serialize};

mod lane_claims;
mod project_state;
mod report;
mod runtime;
mod skills;
mod topology;

use crate::model::{SessionStatusSnapshot, TrackerIssue};
use crate::runtime_state::RuntimeState;
use lane_claims::{audit_lane_claim_fields, claimed_main_agent};
use project_state::{
    audit_project_state_post_claims, audit_project_state_pre_claims, audit_terminal_state_mismatch,
};
use runtime::{audit_runtime_consistency, audit_session_consistency};
use topology::audit_parent_subissue_topology;

pub use report::{
    draft_pr_repair_candidates, human_review_repair_candidates, render_doctor_repair_workpad,
    render_human_review_repair_workpad, render_project_audit_report,
    render_project_audit_report_json,
};
pub use skills::{
    append_local_skill_install_doctor_violations, default_jade_symphony_skill_targets,
    SkillInstallTarget,
};

pub const HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE: &str = "human_review_missing_review_evidence";
pub const AGENT_REVIEW_DRAFT_PR: &str = "agent_review_draft_pr";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditSeverity {
    Warning,
    Blocker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAuditViolation {
    pub issue_ref: String,
    pub title: String,
    pub state: String,
    pub severity: AuditSeverity,
    pub code: String,
    pub message: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAuditReport {
    pub total_issues: usize,
    pub violations: Vec<ProjectAuditViolation>,
    #[serde(default)]
    pub integration_gaps: Vec<String>,
    #[serde(default)]
    pub skill_readiness_summary: Option<String>,
}

impl ProjectAuditReport {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn blocker_count(&self) -> usize {
        self.violations
            .iter()
            .filter(|violation| violation.severity == AuditSeverity::Blocker)
            .count()
    }
}

#[derive(Debug, Clone)]
pub struct ProjectDoctorContext {
    pub runtime_state: Option<RuntimeState>,
    pub runtime_states: Vec<RuntimeState>,
    pub sessions: Vec<SessionStatusSnapshot>,
    pub now_ms: u64,
    pub stale_after_ms: u64,
}

pub fn audit_project_issues(issues: &[TrackerIssue]) -> ProjectAuditReport {
    audit_project_issues_with_context(issues, None)
}

pub fn audit_project_issues_with_context(
    issues: &[TrackerIssue],
    context: Option<&ProjectDoctorContext>,
) -> ProjectAuditReport {
    let mut violations = Vec::new();
    for issue in issues {
        let state = issue.normalized_state();
        audit_terminal_state_mismatch(issue, &state, &mut violations);

        if state == "done" {
            continue;
        }

        audit_project_state_pre_claims(issue, &state, &mut violations);
        audit_lane_claim_fields(issue, &state, context, &mut violations);
        audit_project_state_post_claims(issue, &state, &mut violations);

        if let Some(context) = context {
            audit_runtime_consistency(issue, &state, context, &mut violations);
        }
    }

    audit_parent_subissue_topology(issues, &mut violations);

    if let Some(context) = context {
        audit_session_consistency(issues, context, &mut violations);
    }

    ProjectAuditReport {
        total_issues: issues.len(),
        violations,
        integration_gaps: Vec::new(),
        skill_readiness_summary: None,
    }
}

fn violation(
    issue: &TrackerIssue,
    severity: AuditSeverity,
    code: &str,
    message: &str,
    suggestion: &str,
) -> ProjectAuditViolation {
    ProjectAuditViolation {
        issue_ref: issue.identifier.clone(),
        title: issue.title.clone(),
        state: issue.state.clone(),
        severity,
        code: code.into(),
        message: message.into(),
        suggestion: suggestion.into(),
    }
}

fn find_issue_by_ref<'a>(issues: &'a [TrackerIssue], issue_ref: &str) -> Option<&'a TrackerIssue> {
    issues
        .iter()
        .find(|issue| issue_refs_match(&issue.identifier, issue_ref))
}

fn bool_project_field(issue: &TrackerIssue, key: &str) -> bool {
    issue
        .project_fields
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn string_project_field(issue: &TrackerIssue, key: &str) -> Option<String> {
    issue
        .project_fields
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn issue_refs_match(left: &str, right: &str) -> bool {
    normalize_issue_ref(left) == normalize_issue_ref(right)
}

fn normalize_issue_ref(value: &str) -> String {
    value.trim().trim_start_matches('#').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LinkedPullRequest;
    use std::collections::BTreeMap;

    mod lane_claims;
    mod project_state;
    mod report;
    mod runtime;
    mod skills;
    mod topology;

    fn issue(identifier: &str, state: &str) -> TrackerIssue {
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

    fn linked_pr(url: &str, state: &str) -> LinkedPullRequest {
        LinkedPullRequest {
            id: Some("PR_1".into()),
            number: Some(1),
            url: Some(url.into()),
            state: Some(state.into()),
            ..Default::default()
        }
    }

    fn linked_pr_to(url: &str, state: &str, base: &str) -> LinkedPullRequest {
        let mut pr = linked_pr(url, state);
        pr.base_ref_name = Some(base.into());
        pr
    }

    fn with_native_parent(mut issue: TrackerIssue, parent: &str) -> TrackerIssue {
        issue.project_fields.insert(
            "GitHub Native Parent".into(),
            serde_json::json!({ "identifier": parent }),
        );
        issue
    }

    fn with_native_subissues(mut issue: TrackerIssue, subissues: &[&str]) -> TrackerIssue {
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

    fn with_parent_branch(mut issue: TrackerIssue, branch: &str) -> TrackerIssue {
        issue.description = Some(format!(
            "## Parent Topology\n\nParent integration branch: `{branch}`"
        ));
        issue
    }

    fn session(identifier: Option<&str>, status: &str) -> SessionStatusSnapshot {
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

    fn with_github_issue_state(mut issue: TrackerIssue, state: &str) -> TrackerIssue {
        issue.project_fields.insert(
            "GitHub Issue State".into(),
            serde_json::Value::String(state.into()),
        );
        issue
    }
}
