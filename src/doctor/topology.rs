use crate::model::{normalize_state, TrackerIssue};

use super::{
    bool_project_field, find_issue_by_ref, issue_refs_match, string_project_field, violation,
    AuditSeverity, ProjectAuditViolation,
};

pub(super) fn audit_parent_subissue_topology(
    issues: &[TrackerIssue],
    violations: &mut Vec<ProjectAuditViolation>,
) {
    for issue in issues {
        audit_body_only_parent_hierarchy(issue, violations);
    }

    for parent in issues {
        if parent.normalized_state() == "done" {
            continue;
        }
        let subissue_refs = native_subissue_refs(parent, issues);
        if subissue_refs.is_empty() {
            continue;
        }

        let parent_branches = parent_integration_branch_candidates(parent);
        match parent_branches.as_slice() {
            [] => violations.push(violation(
                parent,
                AuditSeverity::Blocker,
                "parent_topology_missing_integration_branch",
                "Parent issue has native subissues but no parent integration branch evidence.",
                "Record the parent integration branch in the parent issue body or Shea Symphony workpad before subissue PRs advance.",
            )),
            [_] => {}
            branches => violations.push(violation(
                parent,
                AuditSeverity::Blocker,
                "parent_topology_ambiguous_integration_branch",
                &format!(
                    "Parent issue has multiple parent integration branch candidates: {}.",
                    branches.join(", ")
                ),
                "Choose one parent integration branch and supersede stale branch evidence before Human Review or Merge.",
            )),
        }

        let Some(parent_branch) = parent_branches.first() else {
            continue;
        };
        if parent_branches.len() > 1 {
            continue;
        }

        audit_parent_human_review_gate(parent, &subissue_refs, issues, parent_branch, violations);
    }

    for subissue in issues {
        let Some(parent_ref) = native_parent_ref(subissue) else {
            continue;
        };
        let Some(parent) = find_issue_by_ref(issues, &parent_ref) else {
            continue;
        };
        if parent.normalized_state() == "done" {
            continue;
        }
        let parent_branches = parent_integration_branch_candidates(parent);
        let Some(parent_branch) = parent_branches.first() else {
            continue;
        };
        if parent_branches.len() > 1 {
            continue;
        }

        audit_subissue_pr_target(subissue, parent_branch, violations);
        audit_subissue_done_merge_evidence(subissue, parent_branch, violations);
    }
}

fn audit_body_only_parent_hierarchy(
    issue: &TrackerIssue,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    if native_parent_ref(issue).is_some() {
        return;
    }
    if issue.normalized_state() == "done" {
        return;
    }
    let Some(description) = issue.description.as_deref() else {
        return;
    };
    let mut supplemental_parent_refs = supplemental_parent_refs(description);
    supplemental_parent_refs.retain(|issue_ref| !issue_refs_match(issue_ref, &issue.identifier));
    if supplemental_parent_refs.is_empty() {
        return;
    }

    violations.push(violation(
        issue,
        AuditSeverity::Warning,
        "body_only_parent_hierarchy",
        &format!(
            "Issue claims parent membership in text without a GitHub native parent link: {}.",
            supplemental_parent_refs.join(", ")
        ),
        "Create or repair the GitHub native sub-issue relationship, or record an explicit topology exception before relying on parent/subissue flow.",
    ));
}

fn audit_subissue_pr_target(
    subissue: &TrackerIssue,
    parent_branch: &str,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    if has_parent_topology_exception(subissue) {
        return;
    }

    for pr in &subissue.linked_pull_requests {
        let Some(base) = pr.base_ref_name.as_deref() else {
            continue;
        };
        if base == "main" {
            violations.push(violation(
                subissue,
                AuditSeverity::Blocker,
                "subissue_pr_targets_main",
                "Native subissue PR targets `main` instead of the parent integration branch.",
                &format!(
                    "Retarget the subissue PR to `{parent_branch}` or record an explicit parent topology exception in the issue workpad and PR body."
                ),
            ));
        } else if base != parent_branch {
            violations.push(violation(
                subissue,
                AuditSeverity::Warning,
                "subissue_pr_target_mismatch",
                &format!(
                    "Native subissue PR targets `{base}` instead of parent branch `{parent_branch}`."
                ),
                "Inspect the linked PR base branch and parent integration branch evidence before handoff or merge.",
            ));
        }
    }
}

fn audit_subissue_done_merge_evidence(
    subissue: &TrackerIssue,
    parent_branch: &str,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    if subissue.normalized_state() != "done" || has_parent_topology_exception(subissue) {
        return;
    }
    if has_merged_into_parent_branch_evidence(subissue, parent_branch) {
        return;
    }

    violations.push(violation(
        subissue,
        AuditSeverity::Blocker,
        "subissue_done_missing_parent_merge",
        "Subissue is Done without evidence that its PR merged into the parent integration branch.",
        &format!(
            "Keep the subissue out of Done until linked PR evidence or workpad merge evidence shows it merged into `{parent_branch}`."
        ),
    ));
}

fn audit_parent_human_review_gate(
    parent: &TrackerIssue,
    subissue_refs: &[String],
    issues: &[TrackerIssue],
    parent_branch: &str,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    if parent.normalized_state() != "human review" {
        return;
    }

    for subissue_ref in subissue_refs {
        let Some(subissue) = find_issue_by_ref(issues, subissue_ref) else {
            violations.push(violation(
                parent,
                AuditSeverity::Blocker,
                "parent_human_review_missing_subissue",
                &format!(
                    "Parent issue is in Human Review but native subissue `{subissue_ref}` is missing from the Project read."
                ),
                "Refresh Project membership or add the native subissue to the Project before parent Human Review proceeds.",
            ));
            continue;
        };

        if subissue.normalized_state() != "done" {
            violations.push(violation(
                parent,
                AuditSeverity::Blocker,
                "parent_human_review_subissue_not_done",
                &format!(
                    "Parent issue is in Human Review while native subissue `{}` is `{}`.",
                    subissue.identifier, subissue.state
                ),
                "Return the parent issue before Human Review until every native subissue is Done and merged into the parent branch.",
            ));
        } else if !has_merged_into_parent_branch_evidence(subissue, parent_branch)
            && !has_parent_topology_exception(subissue)
        {
            violations.push(violation(
                parent,
                AuditSeverity::Blocker,
                "parent_human_review_subissue_missing_merge",
                &format!(
                    "Parent issue is in Human Review but native subissue `{}` lacks merge evidence into `{parent_branch}`.",
                    subissue.identifier
                ),
                "Record or repair subissue merge evidence before parent Human Review proceeds.",
            ));
        }
    }
}

fn native_parent_ref(issue: &TrackerIssue) -> Option<String> {
    issue
        .project_fields
        .get("GitHub Native Parent")
        .and_then(issue_ref_from_value)
}

fn native_subissue_refs(parent: &TrackerIssue, issues: &[TrackerIssue]) -> Vec<String> {
    let mut refs = Vec::new();
    if let Some(values) = parent
        .project_fields
        .get("GitHub Native Subissues")
        .and_then(serde_json::Value::as_array)
    {
        for value in values {
            if let Some(issue_ref) = issue_ref_from_value(value) {
                push_unique(&mut refs, issue_ref);
            }
        }
    }

    for issue in issues {
        if native_parent_ref(issue)
            .is_some_and(|issue_ref| issue_refs_match(&issue_ref, &parent.identifier))
        {
            push_unique(&mut refs, issue.identifier.clone());
        }
    }

    refs
}

fn issue_ref_from_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("identifier")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| value.as_str().map(str::to_string))
        .or_else(|| {
            value
                .get("number")
                .and_then(serde_json::Value::as_u64)
                .map(|number| format!("#{number}"))
        })
}

fn parent_integration_branch_candidates(issue: &TrackerIssue) -> Vec<String> {
    let mut branches = Vec::new();
    for value in issue.project_fields.values() {
        collect_integration_branches_from_value(value, &mut branches);
    }
    if let Some(description) = issue.description.as_deref() {
        collect_integration_branches(description, &mut branches);
    }
    if let Some(branch) = issue
        .branch_name
        .as_deref()
        .filter(|branch| branch.starts_with("integration/"))
    {
        push_unique(&mut branches, branch.to_string());
    }
    for pr in &issue.linked_pull_requests {
        if let Some(head) = pr
            .head_ref_name
            .as_deref()
            .filter(|branch| branch.starts_with("integration/"))
        {
            push_unique(&mut branches, head.to_string());
        }
    }
    branches
}

fn collect_integration_branches_from_value(value: &serde_json::Value, branches: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => collect_integration_branches(text, branches),
        serde_json::Value::Array(values) => {
            for value in values {
                collect_integration_branches_from_value(value, branches);
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values() {
                collect_integration_branches_from_value(value, branches);
            }
        }
        _ => {}
    }
}

fn collect_integration_branches(text: &str, branches: &mut Vec<String>) {
    for token in text.split(|character: char| {
        character.is_whitespace()
            || matches!(
                character,
                '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';'
            )
    }) {
        let branch = token.trim_matches(|character: char| {
            matches!(character, '.' | ':' | '`' | '"' | '\'' | ')' | ']' | '}')
        });
        if branch.starts_with("integration/issue-") {
            push_unique(branches, branch.to_string());
        }
    }
}

fn supplemental_parent_refs(description: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for line in description.lines() {
        let lower = line.to_lowercase();
        let explicit_subissue_parent_claim = lower.contains("subissue under parent")
            || lower.contains("subissue seed under parent")
            || lower.contains("native parent")
            || lower.contains("native_parent")
            || lower.contains("claimed_parent")
            || (lower.contains("related parent issue") && lower.contains("subissue"));
        if !explicit_subissue_parent_claim {
            continue;
        }
        collect_issue_refs(line, &mut refs);
    }
    refs
}

fn collect_issue_refs(text: &str, refs: &mut Vec<String>) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'#' {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        let digits_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index > digits_start {
            push_unique(refs, text[start..index].to_string());
        }
    }
}

fn has_parent_topology_exception(issue: &TrackerIssue) -> bool {
    issue.description.as_deref().is_some_and(|description| {
        let text = description.to_lowercase();
        text.contains("parent topology exception")
            || text.contains("parent integration branch exception")
            || text.contains("not part of the parent integration branch")
    }) || string_project_field(issue, "parent_topology_exception")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || bool_project_field(issue, "parent_topology_exception_recorded")
}

fn has_merged_into_parent_branch_evidence(issue: &TrackerIssue, parent_branch: &str) -> bool {
    issue.linked_pull_requests.iter().any(|pr| {
        pr.base_ref_name.as_deref() == Some(parent_branch)
            && pr
                .state
                .as_deref()
                .is_some_and(|state| normalize_state(state) == "merged")
    }) || issue.description.as_deref().is_some_and(|description| {
        let text = description.to_lowercase();
        text.contains(&parent_branch.to_lowercase())
            && text.contains("merged")
            && (text.contains("merge evidence") || text.contains("pr "))
    })
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}
