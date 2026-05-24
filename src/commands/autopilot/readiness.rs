use std::path::Path;

use jade_symphony::canonical_checkout::{
    canonical_checkout_status_line, inspect_canonical_checkout, CanonicalCheckoutReport,
};
use jade_symphony::config::RuntimeConfig;
use jade_symphony::doctor::{
    append_local_skill_install_doctor_violations, audit_project_issues_with_context,
    default_jade_symphony_skill_targets, AuditSeverity, ProjectAuditReport, ProjectDoctorContext,
};
use jade_symphony::model::{SessionStatusSnapshot, TrackerIssue};
use jade_symphony::runtime_state::RuntimeState;
use jade_symphony::skill_status::{doctor_skill_readiness_summary, SkillStatusInput};
use serde::Serialize;

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
        stale_after_ms: 10_800_000,
    };
    let mut report = audit_project_issues_with_context(issues, Some(&context));
    report.integration_gaps = integration_gaps;
    append_canonical_checkout_doctor_violations(&mut report, config);
    append_workspace_doctor_violations(&mut report, config, issues);
    if let Ok(skill_repo_root) = discover_skill_suite_repo_root(workflow_path) {
        let skill_targets = default_jade_symphony_skill_targets();
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
    blockers.extend(runtime.blockers.iter().cloned());
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
    } else if !runtime.blockers.is_empty() {
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
                let reason = canonical_checkout_readiness_blocker(&report);
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

fn canonical_checkout_readiness_blocker(report: &CanonicalCheckoutReport) -> Option<String> {
    let Some(branch) = report.branch.as_deref() else {
        return Some("HEAD is detached".into());
    };
    if branch != "main" {
        return Some(format!("current branch is {branch:?}, expected \"main\""));
    }
    if let (Some(head), Some(upstream), Some(upstream_head)) = (
        report.head.as_deref(),
        report.upstream.as_deref(),
        report.upstream_head.as_deref(),
    ) {
        if head != upstream_head {
            return Some(format!(
                "local main does not match upstream {upstream} at {upstream_head}"
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
        runtime_load_error: Option<String>,
        session_load_error: Option<String>,
    ) -> Self {
        let attention_sessions = sessions
            .iter()
            .filter(|session| session_needs_autopilot_attention(session))
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
