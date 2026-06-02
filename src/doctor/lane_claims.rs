use crate::lane_claim::{LaneClaim, LaneClaimState};
use crate::model::TrackerIssue;

use super::{
    string_project_field, violation, AuditSeverity, ProjectAuditViolation, ProjectDoctorContext,
};

pub(super) fn claimed_main_agent(issue: &TrackerIssue) -> Option<String> {
    string_project_field_any(issue, &["Main Agent", "main_agent"])
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn active_claimed_main_agent(issue: &TrackerIssue) -> Option<String> {
    claimed_main_agent(issue).filter(|value| match LaneClaim::parse(value) {
        Ok(claim) => claim.state == LaneClaimState::Active,
        Err(_) => true,
    })
}

pub(super) fn audit_lane_claim_fields(
    issue: &TrackerIssue,
    normalized_issue_state: &str,
    context: Option<&ProjectDoctorContext>,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    for (field, expected_lane) in [
        ("Main Agent", "main"),
        ("Review Agent", "review"),
        ("Merging Agent", "merge"),
    ] {
        let Some(value) = string_project_field(issue, field)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        match LaneClaim::parse(&value) {
            Ok(claim) => {
                if claim.lane.as_str() != expected_lane {
                    violations.push(violation(
                        issue,
                        AuditSeverity::Warning,
                        "lane_claim_mismatched_lane",
                        &format!(
                            "{field} claim has lane `{}` instead of `{expected_lane}`.",
                            claim.lane.as_str()
                        ),
                        "Rewrite the claim through the owning lane so the Project field matches its lane.",
                    ));
                }
                if claim.issue != issue.identifier {
                    violations.push(violation(
                        issue,
                        AuditSeverity::Warning,
                        "lane_claim_mismatched_issue",
                        &format!("{field} claim points at `{}`.", claim.issue),
                        "Preserve the old evidence in the workpad, then write a fresh claim for this issue if work is still active.",
                    ));
                }
                if matches!(normalized_issue_state, "done" | "closed")
                    && claim.state == LaneClaimState::Active
                {
                    violations.push(violation(
                        issue,
                        AuditSeverity::Warning,
                        "terminal_issue_active_lane_claim",
                        &format!("{field} claim is still `state=active` on a terminal issue."),
                        "Update the structured claim to `state=done` after preserving run evidence.",
                    ));
                }
                if claim.state == LaneClaimState::Active
                    && !matches!(normalized_issue_state, "done" | "closed")
                    && context.is_some_and(|context| !context_has_run(context, &claim.run))
                {
                    violations.push(violation(
                        issue,
                        AuditSeverity::Warning,
                        "active_lane_claim_missing_registry",
                        &format!("{field} claim run `{}` has no matching runtime/session registry evidence.", claim.run),
                        "Preserve any issue/worktree/PR context, then use doctor repair or a superseding lane claim before starting replacement work.",
                    ));
                }
                if claim.state.is_terminal_audit_pointer()
                    && !matches!(normalized_issue_state, "done" | "closed")
                    && context.is_some_and(|context| !context_has_run(context, &claim.run))
                {
                    violations.push(violation(
                        issue,
                        AuditSeverity::Warning,
                        "terminal_lane_claim_missing_registry",
                        &format!("{field} terminal claim run `{}` has no matching runtime/session registry evidence.", claim.run),
                        "Treat this as historical audit guidance; preserve the claim and supersede it only if this lane needs fresh work.",
                    ));
                }
            }
            Err(_) if matches!(normalized_issue_state, "done" | "closed") => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Warning,
                    "terminal_issue_legacy_lane_claim",
                    &format!("{field} retains a legacy claim value."),
                    "Keep it as audit evidence for now; migrate it through a future doctor repair flow if needed.",
                ));
            }
            Err(_) => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Warning,
                    "active_issue_legacy_lane_claim",
                    &format!("{field} contains a legacy claim value."),
                    "Inspect the active workspace/session, then supersede it with a structured `v=1` claim before dispatching another worker.",
                ));
            }
        }
    }
}

pub(super) fn audit_terminal_active_lane_claim_fields(
    issue: &TrackerIssue,
    normalized_issue_state: &str,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    if !matches!(normalized_issue_state, "done" | "closed") {
        return;
    }

    for field in ["Main Agent", "Review Agent", "Merging Agent"] {
        let Some(value) = string_project_field(issue, field)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        if LaneClaim::parse(&value)
            .map(|claim| claim.state == LaneClaimState::Active)
            .unwrap_or(false)
        {
            violations.push(violation(
                issue,
                AuditSeverity::Warning,
                "terminal_issue_active_lane_claim",
                &format!("{field} claim is still `state=active` on a terminal issue."),
                "Update the structured claim to `state=done` after preserving run evidence.",
            ));
        }
    }
}

pub(super) fn has_runtime_owner_metadata(issue: &TrackerIssue) -> bool {
    issue.project_fields.contains_key("runtime_owner")
        || issue.project_fields.contains_key("runtime_state")
        || claimed_main_agent(issue).is_some()
        || string_project_field_any(issue, &["Merging Agent", "merging_agent"])
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

fn string_project_field_any(issue: &TrackerIssue, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| string_project_field(issue, key))
}

fn context_has_run(context: &ProjectDoctorContext, run_id: &str) -> bool {
    let runtime_state_has_run = if context.runtime_states.is_empty() {
        context
            .runtime_state
            .iter()
            .any(|state| state.run_id.as_deref() == Some(run_id))
    } else {
        context
            .runtime_states
            .iter()
            .any(|state| state.run_id.as_deref() == Some(run_id))
    };

    runtime_state_has_run
        || context
            .sessions
            .iter()
            .any(|session| session.run_id.as_deref() == Some(run_id))
}
