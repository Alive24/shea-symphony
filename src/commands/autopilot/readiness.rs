use std::path::Path;

use serde::Serialize;
use shea_symphony::canonical_checkout::{
    canonical_checkout_status_line, inspect_canonical_checkout, CanonicalCheckoutReport,
};
use shea_symphony::config::RuntimeConfig;
use shea_symphony::doctor::{
    append_local_skill_install_doctor_violations, audit_project_issues_with_context,
    default_shea_symphony_skill_targets, AuditSeverity, ProjectAuditReport, ProjectDoctorContext,
};
use shea_symphony::model::{normalize_state, SessionStatusSnapshot, TrackerIssue};
use shea_symphony::runtime_state::RuntimeState;
use shea_symphony::skill_status::{doctor_skill_readiness_summary, SkillStatusInput};

use crate::commands::doctor::{
    append_canonical_checkout_doctor_violations, append_workspace_doctor_violations,
    discover_skill_suite_repo_root,
};
use crate::orchestration::current_time_ms;

use super::AutopilotLanePlan;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotReadiness {
    pub(crate) status: String,
    pub(crate) reason: String,
    pub(crate) blockers: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotDoctorSummary {
    pub(crate) blockers: usize,
    pub(crate) warnings: usize,
    pub(crate) blocker_codes: Vec<String>,
    pub(crate) warning_codes: Vec<String>,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotCanonicalCheckout {
    pub(crate) safe_for_write: bool,
    pub(crate) root: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) upstream: Option<String>,
    pub(crate) clean: Option<bool>,
    pub(crate) reason: Option<String>,
    pub(crate) status_line: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotRuntimeSummary {
    pub(crate) runtime_state_count: usize,
    pub(crate) session_count: usize,
    pub(crate) session_attention_count: usize,
    pub(crate) retrying_count: usize,
    pub(crate) active_issues: Vec<AutopilotActiveIssue>,
    pub(crate) retrying: Vec<AutopilotRetryRecord>,
    pub(crate) blockers: Vec<String>,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotActiveIssue {
    pub(crate) lane: String,
    pub(crate) identifier: String,
    pub(crate) backend: String,
    pub(crate) session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotRetryRecord {
    pub(crate) lane: String,
    pub(crate) issue_identifier: Option<String>,
    pub(crate) attempt: u32,
    pub(crate) due_in_ms: u64,
    pub(crate) next_retry_at_ms: u64,
    pub(crate) error: String,
}

pub(crate) fn autopilot_doctor_report(
    workflow_path: &Path,
    config: &RuntimeConfig,
    issues: &[TrackerIssue],
    runtime_states: &[RuntimeState],
    sessions: &[SessionStatusSnapshot],
    integration_gaps: Vec<String>,
) -> ProjectAuditReport {
    let context = ProjectDoctorContext {
        runtime_state: runtime_states.first().cloned(),
        runtime_states: runtime_states.to_vec(),
        sessions: sessions.to_vec(),
        now_ms: current_time_ms(),
        stale_after_ms: config.codex.session_stale_after_ms,
    };
    let mut report = audit_project_issues_with_context(issues, Some(&context));
    report.integration_gaps = integration_gaps;
    append_canonical_checkout_doctor_violations(&mut report, config);
    append_workspace_doctor_violations(&mut report, config, issues);
    if let Ok(skill_repo_root) = discover_skill_suite_repo_root(workflow_path) {
        let skill_targets = default_shea_symphony_skill_targets();
        append_local_skill_install_doctor_violations(&mut report, &skill_repo_root, &skill_targets);
        report.skill_readiness_summary = Some(doctor_skill_readiness_summary(SkillStatusInput {
            workflow_path: workflow_path.to_path_buf(),
            suite_path: None,
            codex_dir: None,
            gemini_dir: None,
            require_gemini: false,
            session_skills: Vec::new(),
            session_skills_file: None,
        }));
    }
    report
}

pub(crate) fn autopilot_readiness(
    lanes: &[AutopilotLanePlan],
    doctor: &AutopilotDoctorSummary,
    canonical_checkout: &AutopilotCanonicalCheckout,
    runtime: &AutopilotRuntimeSummary,
    integration_gaps: &[String],
) -> AutopilotReadiness {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    let runtime_blockers = autopilot_effective_runtime_blockers(lanes, runtime);

    if doctor.blockers > 0 {
        blockers.push(format!("doctor_blockers={}", doctor.blockers));
    }
    if !canonical_checkout.safe_for_write {
        blockers.push(format!(
            "canonical_checkout={}",
            canonical_checkout
                .reason
                .as_deref()
                .unwrap_or("not safe for future write-mode autopilot")
        ));
    }
    blockers.extend(runtime_blockers.iter().cloned());
    warnings.extend(doctor.evidence.iter().take(5).cloned());
    warnings.extend(
        integration_gaps
            .iter()
            .filter(|gap| !gap.contains("canonical_checkout"))
            .take(5)
            .cloned(),
    );

    let active_lane = lanes.iter().any(|lane| lane.status == "ready");
    let (status, reason) = if doctor.blockers > 0 || !canonical_checkout.safe_for_write {
        (
            "blocked_by_doctor_or_canonical_checkout",
            "Doctor blockers or canonical checkout safety must be resolved before write-mode autopilot.",
        )
    } else if !runtime_blockers.is_empty() {
        (
            "blocked_by_ambiguous_lane_or_runtime_state",
            "Runtime/session state needs operator attention before write-mode autopilot.",
        )
    } else if active_lane {
        (
            "ready",
            "At least one lane has dispatchable work and no readiness blocker was found.",
        )
    } else {
        (
            "idle_but_healthy",
            "All lanes are idle and no readiness blocker was found.",
        )
    };

    AutopilotReadiness {
        status: status.into(),
        reason: reason.into(),
        blockers,
        warnings,
    }
}

fn autopilot_effective_runtime_blockers(
    lanes: &[AutopilotLanePlan],
    runtime: &AutopilotRuntimeSummary,
) -> Vec<String> {
    runtime
        .blockers
        .iter()
        .filter(|blocker| {
            let main_runtime_can_continue = blocker.starts_with("active_runtime_states=")
                && autopilot_main_runtime_should_not_block_ready_main(lanes, runtime);
            let terminal_session_history_can_continue = blocker.starts_with("session_attention=")
                && autopilot_terminal_session_history_should_not_block_ready_main(lanes, runtime);
            !(main_runtime_can_continue || terminal_session_history_can_continue)
        })
        .cloned()
        .collect()
}

fn autopilot_main_runtime_should_not_block_ready_main(
    lanes: &[AutopilotLanePlan],
    runtime: &AutopilotRuntimeSummary,
) -> bool {
    !runtime.active_issues.is_empty()
        && runtime
            .active_issues
            .iter()
            .all(|issue| issue.lane.eq_ignore_ascii_case("main"))
        && lanes.iter().any(|lane| {
            lane.lane == "main" && lane.status == "ready" && lane.selected_issue.is_some()
        })
}

fn autopilot_terminal_session_history_should_not_block_ready_main(
    lanes: &[AutopilotLanePlan],
    runtime: &AutopilotRuntimeSummary,
) -> bool {
    if !runtime.active_issues.is_empty()
        || !lanes.iter().any(|lane| {
            lane.lane == "main" && lane.status == "ready" && lane.selected_issue.is_some()
        })
    {
        return false;
    }
    let session_evidence = runtime
        .evidence
        .iter()
        .filter(|line| line.starts_with("session="))
        .collect::<Vec<_>>();
    !session_evidence.is_empty()
        && session_evidence.iter().all(|line| {
            !autopilot_readiness_evidence_field_equals(line, "issue", "unknown")
                && (autopilot_readiness_evidence_field_equals(line, "status", "failed")
                    || autopilot_readiness_evidence_field_equals(line, "status", "stale"))
        })
}

fn autopilot_readiness_evidence_field_equals(line: &str, key: &str, expected: &str) -> bool {
    let prefix = format!("{key}=");
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&prefix))
        .is_some_and(|value| value == expected)
}

impl AutopilotDoctorSummary {
    pub(crate) fn from_report(report: &ProjectAuditReport) -> Self {
        let mut blocker_codes = Vec::new();
        let mut warning_codes = Vec::new();
        let mut evidence = Vec::new();
        for violation in &report.violations {
            match violation.severity {
                AuditSeverity::Blocker => blocker_codes.push(violation.code.clone()),
                AuditSeverity::Warning => warning_codes.push(violation.code.clone()),
            }
            evidence.push(format!(
                "{} severity={:?} code={} message={}",
                violation.issue_ref, violation.severity, violation.code, violation.message
            ));
        }
        if let Some(summary) = &report.skill_readiness_summary {
            evidence.push(summary.clone());
        }
        Self {
            blockers: report.blocker_count(),
            warnings: report
                .violations
                .iter()
                .filter(|violation| violation.severity == AuditSeverity::Warning)
                .count(),
            blocker_codes,
            warning_codes,
            evidence,
        }
    }
}

impl AutopilotCanonicalCheckout {
    pub(crate) fn read_current(config: &RuntimeConfig) -> Self {
        let root = match std::env::current_dir() {
            Ok(root) => root,
            Err(error) => {
                return Self::blocked(format!("current directory unavailable: {error}"));
            }
        };
        match inspect_canonical_checkout(&root, config) {
            Ok(report) => {
                let reason =
                    canonical_checkout_readiness_blocker(&report, config.git_base_branch());
                Self {
                    safe_for_write: reason.is_none(),
                    root: Some(report.root.display().to_string()),
                    branch: report.branch.clone(),
                    upstream: report.upstream.clone(),
                    clean: Some(report.is_clean()),
                    reason,
                    status_line: Some(canonical_checkout_status_line(&report)),
                }
            }
            Err(error) => Self::blocked(error.to_string()),
        }
    }

    fn blocked(reason: String) -> Self {
        Self {
            safe_for_write: false,
            root: None,
            branch: None,
            upstream: None,
            clean: None,
            reason: Some(reason),
            status_line: None,
        }
    }
}

fn canonical_checkout_readiness_blocker(
    report: &CanonicalCheckoutReport,
    base_branch: &str,
) -> Option<String> {
    let Some(branch) = report.branch.as_deref() else {
        return Some("HEAD is detached".into());
    };
    if branch != base_branch {
        return Some(format!(
            "current branch is {branch:?}, expected \"{base_branch}\""
        ));
    }
    if let (Some(head), Some(upstream), Some(upstream_head)) = (
        report.head.as_deref(),
        report.upstream.as_deref(),
        report.upstream_head.as_deref(),
    ) {
        if head != upstream_head {
            return Some(format!(
                "local {base_branch} does not match upstream {upstream} at {upstream_head}"
            ));
        }
    }
    if !report.tracked_dirty.is_empty() {
        return Some(format!(
            "tracked dirty files: {}",
            report.tracked_dirty.join(", ")
        ));
    }
    let unclassified = report
        .unclassified_untracked()
        .iter()
        .map(|entry| entry.path.display().to_string())
        .collect::<Vec<_>>();
    if !unclassified.is_empty() {
        return Some(format!(
            "unclassified untracked files: {}",
            unclassified.join(", ")
        ));
    }
    None
}

impl AutopilotRuntimeSummary {
    pub(crate) fn from_parts(
        runtime_states: &[RuntimeState],
        sessions: &[SessionStatusSnapshot],
        issues: &[TrackerIssue],
        runtime_load_error: Option<String>,
        session_load_error: Option<String>,
    ) -> Self {
        let attention_sessions = sessions
            .iter()
            .filter(|session| {
                session_needs_autopilot_attention(session)
                    && !session_points_at_terminal_issue(session, issues)
            })
            .collect::<Vec<_>>();
        let mut blockers = Vec::new();
        if let Some(error) = runtime_load_error {
            blockers.push(format!("runtime_state_load_error={error}"));
        }
        if let Some(error) = session_load_error {
            blockers.push(format!("session_status_load_error={error}"));
        }
        if !runtime_states.is_empty() {
            blockers.push(format!("active_runtime_states={}", runtime_states.len()));
        }
        if !attention_sessions.is_empty() {
            blockers.push(format!("session_attention={}", attention_sessions.len()));
        }
        let now_ms = current_time_ms();
        let active_issues = runtime_states
            .iter()
            .filter_map(|state| {
                state
                    .active_issue
                    .as_ref()
                    .map(|issue| AutopilotActiveIssue {
                        lane: state.lane.clone().unwrap_or_else(|| "unknown".into()),
                        identifier: issue.identifier.clone(),
                        backend: state.backend.clone(),
                        session_id: state.backend_session_id.clone(),
                    })
            })
            .collect::<Vec<_>>();
        let retrying = runtime_states
            .iter()
            .filter_map(|state| {
                let retry = state.retry.as_ref()?;
                Some(AutopilotRetryRecord {
                    lane: state.lane.clone().unwrap_or_else(|| "unknown".into()),
                    issue_identifier: state
                        .active_issue
                        .as_ref()
                        .map(|issue| issue.identifier.clone()),
                    attempt: retry.attempt,
                    due_in_ms: retry.due_in_ms(now_ms),
                    next_retry_at_ms: retry.next_retry_at_ms,
                    error: retry.error.clone(),
                })
            })
            .collect::<Vec<_>>();
        let mut evidence = runtime_states
            .iter()
            .filter_map(|state| {
                state.active_issue.as_ref().map(|issue| {
                    format!(
                        "runtime issue={} lane={} backend={} session={}",
                        issue.identifier,
                        state.lane.as_deref().unwrap_or("unknown"),
                        state.backend,
                        state.backend_session_id.as_deref().unwrap_or("none")
                    )
                })
            })
            .collect::<Vec<_>>();
        evidence.extend(attention_sessions.iter().map(|session| {
            format!(
                "session={} lane={} status={} issue={}",
                session.session_id,
                session.lane,
                session.status,
                session.issue_identifier.as_deref().unwrap_or("unknown")
            )
        }));
        Self {
            runtime_state_count: runtime_states.len(),
            session_count: sessions.len(),
            session_attention_count: attention_sessions.len(),
            retrying_count: retrying.len(),
            active_issues,
            retrying,
            blockers,
            evidence,
        }
    }
}

fn session_needs_autopilot_attention(session: &SessionStatusSnapshot) -> bool {
    matches!(
        session.status.as_str(),
        "waiting_for_approval"
            | "waiting_for_human_input"
            | "waiting_for_trust"
            | "usage_limited"
            | "failed"
            | "stale"
    )
}

fn session_points_at_terminal_issue(
    session: &SessionStatusSnapshot,
    issues: &[TrackerIssue],
) -> bool {
    let Some(session_issue) = session.issue_identifier.as_deref() else {
        return false;
    };
    issues.iter().any(|issue| {
        issue_refs_match(&issue.identifier, session_issue)
            && (matches!(normalize_state(&issue.state).as_str(), "done" | "closed")
                || issue
                    .project_fields
                    .get("GitHub Issue State")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|state| normalize_state(state) == "closed"))
    })
}

fn issue_refs_match(left: &str, right: &str) -> bool {
    normalize_issue_ref(left) == normalize_issue_ref(right)
}

fn normalize_issue_ref(value: &str) -> String {
    value.trim().trim_start_matches('#').to_string()
}
