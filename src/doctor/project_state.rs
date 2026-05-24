use crate::model::{normalize_state, TrackerIssue};

use super::{
    bool_project_field,
    lane_claims::{active_claimed_main_agent, has_runtime_owner_metadata},
    string_project_field, violation, AuditSeverity, ProjectAuditViolation, AGENT_REVIEW_DRAFT_PR,
    HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE,
};

pub(super) fn audit_project_state_pre_claims(
    issue: &TrackerIssue,
    normalized_issue_state: &str,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    match normalized_issue_state {
        "agent review" if !has_pr_url(issue) && !has_handoff_evidence(issue) => {
            violations.push(violation(
                issue,
                AuditSeverity::Blocker,
                "agent_review_missing_pr_handoff",
                "Agent Review issue has no linked PR URL or handoff evidence.",
                "Move back to Rework or Need Human Input with a workpad diagnostic, or repair the missing PR link.",
            ));
        }
        "agent review" if has_draft_pr(issue) => {
            violations.push(violation(
                issue,
                AuditSeverity::Blocker,
                AGENT_REVIEW_DRAFT_PR,
                "Agent Review issue has a linked draft PR.",
                "Confirm handoff evidence, then run `doctor repair <issue> --mark-pr-ready --confirm-handoff-ready --write`; auto-fix will not mark PRs ready.",
            ));
        }
        "human review" if !has_review_pass_evidence(issue) => {
            violations.push(violation(
                issue,
                AuditSeverity::Blocker,
                HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE,
                "Human Review issue has no independent review pass evidence.",
                "Return to Agent Review until Review Agent pass evidence is recorded.",
            ));
        }
        "merging" if reliable_pr_targets(issue).is_empty() => {
            violations.push(violation(
                issue,
                AuditSeverity::Blocker,
                "merging_missing_pr_target",
                "Merging issue has no reliable PR target.",
                "Record exactly one PR link in the Project field, issue closing reference, or Jade Symphony workpad before attempting to land.",
            ));
        }
        "merging" if reliable_pr_targets(issue).len() > 1 => {
            violations.push(violation(
                issue,
                AuditSeverity::Blocker,
                "merging_ambiguous_pr_target",
                "Merging issue has multiple candidate PR targets.",
                "Choose the correct PR and remove or supersede stale PR evidence before attempting to land.",
            ));
        }
        "merging" if has_dirty_or_conflicted_pr(issue) => {
            violations.push(violation(
                issue,
                AuditSeverity::Blocker,
                "merging_pr_not_clean",
                "Merging issue has a dirty, conflicted, or stale PR.",
                "Move to Rework with review freshness evidence before attempting to land.",
            ));
        }
        "in progress" if !has_runtime_owner_metadata(issue) => {
            violations.push(violation(
                issue,
                AuditSeverity::Warning,
                "in_progress_missing_runtime_owner",
                "In Progress issue has no visible runtime ownership metadata.",
                "Confirm the active workspace/session before dispatching another worker.",
            ));
        }
        "todo" | "need to clarify"
            if has_pr_url(issue) && !issue.project_fields.contains_key("pr_status_explanation") =>
        {
            violations.push(violation(
                issue,
                AuditSeverity::Warning,
                "queued_issue_has_pr",
                "Queued or clarification issue already has a linked PR without explanation.",
                "Add workpad context or move the issue to the state matching the PR.",
            ));
        }
        _ => {}
    }
}

pub(super) fn audit_project_state_post_claims(
    issue: &TrackerIssue,
    normalized_issue_state: &str,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    if normalized_issue_state == "todo" && active_claimed_main_agent(issue).is_some() {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "todo_main_agent_claimed",
            "Todo issue already has a Main Agent claim marker.",
            "Treat it as partially claimed or interrupted work; inspect with `doctor repair <issue>` before dispatching another worker.",
        ));
    }

    if normalized_issue_state == "in progress" && has_pr_url(issue) {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "in_progress_has_pr_evidence",
            "In Progress issue already has PR evidence.",
            "Inspect whether the work should be handed off to Agent Review or moved to Need Human Input with a workpad diagnostic.",
        ));
    }
}

pub(super) fn audit_terminal_state_mismatch(
    issue: &TrackerIssue,
    normalized_issue_state: &str,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    let Some(github_issue_state) =
        string_project_field(issue, "GitHub Issue State").map(|value| normalize_state(&value))
    else {
        return;
    };

    if github_issue_state == "closed" && normalized_issue_state != "done" {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "closed_issue_not_done",
            "GitHub issue is closed, but Project Status is not Done.",
            "Reconcile the Project status with the closed GitHub issue before relying on tracker health.",
        ));
    }

    if normalized_issue_state == "done" && github_issue_state != "closed" {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "done_project_issue_still_open",
            "Project Status is Done, but the GitHub issue is still open.",
            "Close the GitHub issue or move the Project item back to the appropriate active state.",
        ));
    }
}

pub(super) fn has_pr_url(issue: &TrackerIssue) -> bool {
    issue
        .linked_pull_requests
        .iter()
        .any(|pr| pr.url.as_deref().is_some_and(|url| !url.trim().is_empty()))
}

pub(super) fn reliable_pr_targets(issue: &TrackerIssue) -> Vec<String> {
    let mut targets = Vec::new();
    for pr in &issue.linked_pull_requests {
        let target = pr
            .url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .map(str::to_string)
            .or_else(|| pr.number.map(|number| format!("#{number}")));
        if let Some(target) = target {
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
    }
    targets
}

fn has_draft_pr(issue: &TrackerIssue) -> bool {
    issue
        .linked_pull_requests
        .iter()
        .any(|pr| pr.is_draft == Some(true))
}

fn has_handoff_evidence(issue: &TrackerIssue) -> bool {
    bool_project_field(issue, "handoff_evidence_recorded")
        || string_project_field(issue, "handoff_evidence")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

fn has_review_pass_evidence(issue: &TrackerIssue) -> bool {
    bool_project_field(issue, "review_pass_evidence_recorded")
        || string_project_field(issue, "review_pass_evidence")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        || issue
            .description
            .as_deref()
            .is_some_and(has_review_pass_evidence_text)
}

fn has_review_pass_evidence_text(description: &str) -> bool {
    let normalized = description.to_lowercase();
    [
        "review pass evidence: `recorded`",
        "review pass evidence: recorded",
        "evidence recorded. independent review agent may move this issue to human review; the main implementation agent must not.",
        "independent agent review passed with recorded evidence; issue is ready for human review.",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn has_dirty_or_conflicted_pr(issue: &TrackerIssue) -> bool {
    string_project_field(issue, "pr_merge_state")
        .map(|state| {
            let state = normalize_state(&state);
            state == "dirty" || state == "blocked" || state == "behind" || state == "conflicted"
        })
        .unwrap_or(false)
}
