use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::TrackerIssue;

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
    let mut lines = vec![
        "## Jade Symphony Doctor Triage".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `doctor`".to_string(),
        "- Actor role: `doctor`".to_string(),
        "- Actor: `jade-symphony doctor`".to_string(),
        "- Run ID: `doctor-repair`".to_string(),
        format!("- Input state: `{}`", issue.state),
        format!(
            "- Target state after repair: `{}`",
            doctor_target_state(action)
        ),
        format!("- Result: `{}`", doctor_result(action)),
        format!("- Requested action: `{action}`"),
    ];

    if let Some(main_agent) = claimed_main_agent(issue) {
        lines.push(format!("- Main Agent: `{main_agent}`"));
    }
    if let Some(branch) = issue.branch_name.as_deref() {
        lines.push(format!("- Tracker branch: `{branch}`"));
    }
    if has_pr_url(issue) {
        let targets = reliable_pr_targets(issue).join(", ");
        lines.push(format!("- PR evidence: `{targets}`"));
    } else {
        lines.push("- PR evidence: `not recorded`".into());
    }
    lines.push(format!(
        "- Evidence summary: {} issue-specific doctor finding(s) captured before repair.",
        related.len()
    ));

    lines.extend([String::new(), "### Doctor Findings".to_string()]);
    if related.is_empty() {
        lines.push("- No issue-specific doctor violations were found.".to_string());
    } else {
        for violation in related {
            lines.push(format!(
                "- `{}` ({:?}): {}",
                violation.code, violation.severity, violation.message
            ));
        }
    }

    lines.extend([
        String::new(),
        "### State Boundary".to_string(),
        "- Doctor repair records evidence before any tracker mutation.".to_string(),
        "- This repair does not delete worktrees, discard local work, or bypass review/merge lane authority.".to_string(),
    ]);

    lines.join("\n")
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
    [
        "## Jade Symphony Doctor Triage".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", violation.issue_ref, violation.title),
        "- Lane: `doctor`".into(),
        "- Actor role: `doctor`".into(),
        "- Actor: `jade-symphony doctor`".into(),
        "- Run ID: `doctor-human-review-repair`".into(),
        format!("- Input state: `{}`", violation.state),
        "- Target state after repair: `Agent Review`".into(),
        "- Result: `repair_recorded`".into(),
        "- PR evidence: `not recorded`".into(),
        format!("- Violation: `{}`", violation.code),
        format!("- Previous state: `{}`", violation.state),
        format!("- Message: {}", violation.message),
        format!("- Repair: {}", violation.suggestion),
        "- Evidence summary: invalid Human Review boundary repair evidence recorded before tracker mutation.".to_string(),
        String::new(),
        "### State Boundary".to_string(),
        "- Main implementation agent is moving this issue back to `Agent Review`.".to_string(),
        "- This repair does not set `Human Review`; that state requires independent Review Agent pass evidence.".to_string(),
    ]
    .join("\n")
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
