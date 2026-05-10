use crate::model::{GateDecision, GateDecisionKind, TrackerIssue};

const REQUIRED_SECTIONS: &[(&str, &str)] = &[
    ("Issue Goal", "goal"),
    ("Why Now", "why now"),
    ("Issue Context", "context"),
    ("Scope", "scope"),
    ("Canonical References", "canonical references"),
    ("Target Repository / Package", "target repo or package"),
    ("Non-Negotiable Guardrails", "guardrails and constraints"),
    ("Verification", "validation requirements"),
    ("Completion Criteria", "acceptance criteria"),
];

pub fn evaluate_issue(issue: &TrackerIssue) -> GateDecision {
    let Some(description) = issue.description.as_deref() else {
        return GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing: vec!["description with executable issue contract".into()],
            assumptions: Vec::new(),
            notes: vec!["Issue body is empty.".into()],
        };
    };

    let mut missing = Vec::new();
    for (heading, label) in REQUIRED_SECTIONS {
        if !contains_heading(description, heading) {
            missing.push((*label).to_string());
        }
    }

    if description.to_lowercase().contains("blocked") && missing.is_empty() {
        return GateDecision {
            kind: GateDecisionKind::Blocked,
            missing: vec!["blocked dependency must be resolved before dispatch".into()],
            assumptions: Vec::new(),
            notes: Vec::new(),
        };
    }

    if !missing.is_empty() {
        return GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing,
            assumptions: Vec::new(),
            notes: vec!["Issue contract does not yet match the Jade quality template.".into()],
        };
    }

    let assumptions = extract_assumptions(description);
    if assumptions.is_empty() {
        GateDecision::ready()
    } else {
        GateDecision {
            kind: GateDecisionKind::ReadyWithAssumptions,
            missing: Vec::new(),
            assumptions,
            notes: Vec::new(),
        }
    }
}

fn contains_heading(markdown: &str, heading: &str) -> bool {
    markdown.lines().any(|line| {
        let line = line.trim_start_matches('#').trim();
        line.eq_ignore_ascii_case(heading)
    })
}

fn extract_assumptions(markdown: &str) -> Vec<String> {
    let mut in_assumptions = false;
    let mut assumptions = Vec::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        let heading = trimmed.trim_start_matches('#').trim();

        if heading.eq_ignore_ascii_case("Assumptions") {
            in_assumptions = true;
            continue;
        }

        if in_assumptions && trimmed.starts_with('#') {
            break;
        }

        if in_assumptions && trimmed.starts_with('-') {
            assumptions.push(trimmed.trim_start_matches('-').trim().to_string());
        }
    }

    assumptions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrackerIssue;

    fn issue(description: Option<String>) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: "1".into(),
            item_id: None,
            identifier: "#1".into(),
            title: "Implement".into(),
            description,
            url: None,
            state: "Todo".into(),
            labels: vec![],
            assignees: vec![],
            priority: None,
            branch_name: None,
            linked_pull_requests: vec![],
            blocked_by: vec![],
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn missing_contract_needs_clarification() {
        let decision = evaluate_issue(&issue(Some("thin body".into())));
        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision.missing.iter().any(|item| item == "goal"));
    }

    #[test]
    fn template_shaped_issue_is_ready() {
        let body = [
            "## Issue Goal",
            "Ship a thing.",
            "## Why Now",
            "It blocks the next slice.",
            "## Issue Context",
            "Context.",
            "## Non-Negotiable Guardrails",
            "- Keep tracker abstraction.",
            "## Scope",
            "### In Scope",
            "- Code.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/jade-symphony",
            "## Verification",
            "### Completion Criteria",
            "- Tests pass.",
        ]
        .join("\n");

        assert!(evaluate_issue(&issue(Some(body))).is_dispatchable());
    }
}
