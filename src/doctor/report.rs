use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::TrackerIssue;
use crate::workpad_templates::{render_workpad_template, WorkpadTemplateId};

use super::{
    claimed_main_agent, issue_refs_match,
    project_state::{has_pr_url, reliable_pr_targets},
    AuditSeverity, ProjectAuditReport, ProjectAuditViolation, AGENT_REVIEW_DRAFT_PR,
    HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE,
};

pub fn render_doctor_repair_workpad(
    issue: &TrackerIssue,
    report: &ProjectAuditReport,
    action: &str,
) -> String {
    let related = related_violations(report, &issue.identifier);
    let mut extra_lines = Vec::new();

    if let Some(main_agent) = claimed_main_agent(issue) {
        extra_lines.push(format!("- Main Agent: `{main_agent}`"));
    }
    if let Some(branch) = issue.branch_name.as_deref() {
        extra_lines.push(format!("- Tracker branch: `{branch}`"));
    }
    if has_pr_url(issue) {
        let targets = reliable_pr_targets(issue).join(", ");
        extra_lines.push(format!("- PR evidence: `{targets}`"));
    } else {
        extra_lines.push("- PR evidence: `not recorded`".into());
    }

    let doctor_findings = if related.is_empty() {
        "- No issue-specific doctor violations were found.".to_string()
    } else {
        related
            .iter()
            .map(|violation| {
                format!(
                    "- `{}` ({:?}): {}",
                    violation.code, violation.severity, violation.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    render_workpad_template(
        None,
        WorkpadTemplateId::DoctorTriage,
        &[
            ("generated_at", current_gmt_timestamp()),
            ("issue_ref", issue.identifier.clone()),
            ("issue_title", issue.title.clone()),
            ("run_id", "doctor-repair".into()),
            ("input_state", issue.state.clone()),
            ("target_state", doctor_target_state(action).into()),
            ("result", doctor_result(action).into()),
            ("action", action.into()),
            ("extra_lines", extra_lines.join("\n")),
            (
                "evidence_summary",
                format!(
                    "{} issue-specific doctor finding(s) captured before repair.",
                    related.len()
                ),
            ),
            ("doctor_findings", doctor_findings),
        ],
    )
}

pub fn render_project_audit_report(report: &ProjectAuditReport) -> String {
    let mut lines = vec![
        format!("project_doctor=ok issues={}", report.total_issues),
        format!("violations={}", report.violations.len()),
        format!("blockers={}", report.blocker_count()),
    ];
    if let Some(summary) = &report.skill_readiness_summary {
        lines.push(format!("skill_readiness_summary={summary}"));
    }

    if report.is_clean() {
        lines.push("summary=Project invariants look clean.".into());
    } else {
        lines.push("summary=Project invariants need attention.".into());
        for violation in &report.violations {
            lines.push(format!(
                "- {} {} state={} severity={:?} code={} message={}",
                violation.issue_ref,
                violation.title,
                violation.state,
                violation.severity,
                violation.code,
                violation.message
            ));
            lines.push(format!("  suggestion={}", violation.suggestion));
        }
    }

    for gap in &report.integration_gaps {
        lines.push(format!("integration_gap={gap}"));
    }

    lines.join("\n")
}

pub fn human_review_repair_candidates(report: &ProjectAuditReport) -> Vec<&ProjectAuditViolation> {
    report
        .violations
        .iter()
        .filter(|violation| {
            violation.severity == AuditSeverity::Blocker
                && violation.code == HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE
        })
        .collect()
}

pub fn draft_pr_repair_candidates(report: &ProjectAuditReport) -> Vec<&ProjectAuditViolation> {
    report
        .violations
        .iter()
        .filter(|violation| {
            violation.severity == AuditSeverity::Blocker && violation.code == AGENT_REVIEW_DRAFT_PR
        })
        .collect()
}

pub fn render_human_review_repair_workpad(violation: &ProjectAuditViolation) -> String {
    render_workpad_template(
        None,
        WorkpadTemplateId::HumanReviewRepair,
        &[
            ("generated_at", current_gmt_timestamp()),
            ("issue_ref", violation.issue_ref.clone()),
            ("issue_title", violation.title.clone()),
            ("input_state", violation.state.clone()),
            ("violation_code", violation.code.clone()),
            ("message", violation.message.clone()),
            ("repair", violation.suggestion.clone()),
        ],
    )
}

pub fn render_project_audit_report_json(
    report: &ProjectAuditReport,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

fn doctor_target_state(action: &str) -> &'static str {
    match action {
        "move_need_human_input" => "Need Human Input",
        "mark_pr_ready" => "Agent Review",
        _ => "unchanged",
    }
}

fn doctor_result(action: &str) -> &'static str {
    match action {
        "move_need_human_input" => "routed",
        "mark_pr_ready" => "repair_recorded",
        _ => "triage_recorded",
    }
}

fn current_gmt_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format_gmt_timestamp(seconds)
}

fn format_gmt_timestamp(seconds_since_unix_epoch: u64) -> String {
    let days = (seconds_since_unix_epoch / 86_400) as i64;
    let seconds_of_day = seconds_since_unix_epoch % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} GMT")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_unix_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

fn related_violations<'a>(
    report: &'a ProjectAuditReport,
    issue_ref: &str,
) -> Vec<&'a ProjectAuditViolation> {
    report
        .violations
        .iter()
        .filter(|violation| issue_refs_match(&violation.issue_ref, issue_ref))
        .collect()
}
