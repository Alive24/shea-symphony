use serde::{Deserialize, Serialize};

use crate::model::{GateDecision, GateDecisionKind, TrackerIssue};
use crate::quality_gate::evaluate_issue;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueForgeMode {
    Discover,
    Discuss,
    Draft,
    Validate,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateClassification {
    Ready,
    ReadyWithAssumptions,
    NeedToClarify,
    TooBroad,
    Blocked,
    DuplicateAlreadyCovered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCandidate {
    pub title: String,
    pub classification: CandidateClassification,
    pub rationale: String,
    #[serde(default)]
    pub follow_up_candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarificationQuestion {
    pub question: String,
    pub why_it_matters: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeValidationReport {
    pub title: String,
    pub decision: GateDecision,
    pub question: Option<ClarificationQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairReport {
    pub validation: ForgeValidationReport,
    pub repaired_markdown: String,
}

pub fn discover_candidates(intent: &str) -> Vec<IssueCandidate> {
    let title = title_from_intent(intent);
    let classification = if intent.split_whitespace().count() >= 5 {
        CandidateClassification::ReadyWithAssumptions
    } else {
        CandidateClassification::NeedToClarify
    };

    vec![IssueCandidate {
        title,
        classification,
        rationale: "Derived from local operator intent; validate or repair before dispatch.".into(),
        follow_up_candidates: Vec::new(),
    }]
}

pub fn next_clarification_question(decision: &GateDecision) -> Option<ClarificationQuestion> {
    if !matches!(
        decision.kind,
        GateDecisionKind::NeedToClarify | GateDecisionKind::TooBroad | GateDecisionKind::Blocked
    ) {
        return None;
    }

    decision.missing.first().map(|missing| ClarificationQuestion {
        question: format!("What is the smallest concrete detail that resolves `{missing}` for this issue?"),
        why_it_matters: "Jade Symphony needs this before dispatch so the agent does not invent execution intent.".into(),
    })
}

pub fn validate_markdown(title: &str, markdown: &str) -> ForgeValidationReport {
    let issue = forge_issue(title, markdown);
    let decision = evaluate_issue(&issue);
    let question = next_clarification_question(&decision);
    ForgeValidationReport {
        title: title.to_string(),
        decision,
        question,
    }
}

pub fn repair_markdown(title: &str, markdown: &str) -> RepairReport {
    let validation = validate_markdown(title, markdown);
    let repaired_markdown = if validation.decision.is_dispatchable() {
        record_gate_notes(markdown, &validation.decision)
    } else {
        repaired_draft(title, markdown, &validation.decision)
    };

    RepairReport {
        validation,
        repaired_markdown,
    }
}

pub fn draft_from_template(title: &str, goal: &str) -> String {
    format!(
        r#"## Issue Setup

- UAT Required: No
- Related Parent Issue or Context: 

## Issue Goal

{goal}

## Why Now

TBD

## Issue Context

TBD

## Decisions / Assumptions

### Decisions

- TBD

### Assumptions

- TBD

## Non-Negotiable Guardrails

- Keep Jade Symphony orchestration infrastructure separate from downstream product business logic.

## Scope

### In Scope

- {title}

### Out of Scope

- Unrelated product business logic.

## Canonical References

### Target Repository / Package

- TBD

### Relevant Knowledge Sources

- TBD

### Relevant Code Paths

- TBD

## Current State

TBD

## Deliverable Shape

TBD

## Risks or Constraints

- TBD

## Expected Outcome

TBD

## Verification

### Completion Criteria

- TBD

### Functional Verification

- TBD

### UAT

- Not required unless the issue becomes operator-observable.

### Context Verification

- Confirm the issue still matches canonical sources.
"#
    )
}

fn title_from_intent(intent: &str) -> String {
    let trimmed = intent.trim();
    if trimmed.is_empty() {
        return "Clarify issue intent".into();
    }

    let words = trimmed
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ");
    format!("Issue candidate: {words}")
}

fn forge_issue(title: &str, markdown: &str) -> TrackerIssue {
    TrackerIssue {
        tracker_kind: "issue_forge".into(),
        id: "forge-local".into(),
        item_id: None,
        identifier: "forge-local".into(),
        title: title.to_string(),
        description: Some(markdown.to_string()),
        url: None,
        state: "Todo".into(),
        labels: Vec::new(),
        assignees: Vec::new(),
        priority: None,
        branch_name: None,
        linked_pull_requests: Vec::new(),
        blocked_by: Vec::new(),
        project_fields: Default::default(),
        created_at: None,
        updated_at: None,
    }
}

fn record_gate_notes(markdown: &str, decision: &GateDecision) -> String {
    let mut repaired = markdown.trim_end().to_string();
    repaired.push_str("\n\n## Issue Forge Gate Notes\n\n");
    repaired.push_str(&format!("- Gate Decision: {:?}\n", decision.kind));
    if decision.assumptions.is_empty() {
        repaired.push_str("- Assumptions: None recorded.\n");
    } else {
        for assumption in &decision.assumptions {
            repaired.push_str(&format!("- Assumption: {assumption}\n"));
        }
    }
    repaired
}

fn repaired_draft(title: &str, markdown: &str, decision: &GateDecision) -> String {
    let goal = extract_first_sentence(markdown)
        .filter(|value| !value.is_empty())
        .unwrap_or(title);
    let mut draft = draft_from_template(title, goal);
    draft = draft.replace(
        "## Why Now\n\nTBD",
        "## Why Now\n\nThis issue needs clarification or structure before it can be safely dispatched.",
    );
    draft = draft.replace(
        "## Issue Context\n\nTBD",
        &format!(
            "## Issue Context\n\nSource input captured by Issue Forge:\n\n```md\n{}\n```",
            markdown.trim()
        ),
    );
    draft = draft.replace(
        "### Decisions\n\n- TBD",
        "### Decisions\n\n- Use Issue Forge repair to convert rough input into the Jade quality template.",
    );
    let assumptions = if decision.missing.is_empty() {
        "- Existing source input is sufficient for an initial executable draft.".to_string()
    } else {
        format!(
            "- Repaired draft still needs human confirmation for: {}.",
            decision.missing.join(", ")
        )
    };
    draft = draft.replace(
        "### Assumptions\n\n- TBD",
        &format!("### Assumptions\n\n{assumptions}"),
    );
    draft = draft.replace(
        "### Target Repository / Package\n\n- TBD",
        "### Target Repository / Package\n\n- Alive24/jade-symphony",
    );
    draft = draft.replace(
        "### Relevant Knowledge Sources\n\n- TBD",
        "### Relevant Knowledge Sources\n\n- docs/bootstrap/JADE_SYMPHONY_SPEC.md\n- docs/bootstrap/JADE_WORKFLOW.md\n- docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md",
    );
    draft = draft.replace(
        "### Relevant Code Paths\n\n- TBD",
        "### Relevant Code Paths\n\n- TBD by implementer after source scan.",
    );
    draft = draft.replace(
        "## Current State\n\nTBD",
        "## Current State\n\nRough issue input exists and has been repaired into the Jade issue contract shape.",
    );
    draft = draft.replace(
        "## Deliverable Shape\n\nTBD",
        "## Deliverable Shape\n\nCode, docs, issue update, or validation artifact as appropriate for the final accepted scope.",
    );
    draft = draft.replace(
        "## Risks or Constraints\n\n- TBD",
        "## Risks or Constraints\n\n- Do not expand scope beyond the repaired issue contract without creating follow-up candidates.",
    );
    draft = draft.replace(
        "## Expected Outcome\n\nTBD",
        "## Expected Outcome\n\nAn executable issue contract that can pass the Issue Quality Gate before dispatch.",
    );
    draft = draft.replace(
        "### Completion Criteria\n\n- TBD",
        "### Completion Criteria\n\n- Issue contract has goal, scope, guardrails, references, and validation requirements.",
    );
    draft = draft.replace(
        "### Functional Verification\n\n- TBD",
        "### Functional Verification\n\n- Run `jade-symphony forge-validate` on the repaired draft.",
    );
    draft
}

fn extract_first_sentence(markdown: &str) -> Option<&str> {
    markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GateDecision, GateDecisionKind};

    #[test]
    fn asks_one_missing_item() {
        let question = next_clarification_question(&GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing: vec!["goal".into(), "scope".into()],
            assumptions: vec![],
            notes: vec![],
        })
        .unwrap();

        assert!(question.question.contains("goal"));
        assert!(!question.question.contains("scope"));
    }

    #[test]
    fn validates_thin_markdown_with_actionable_question() {
        let report = validate_markdown("Thin issue", "make forge better");

        assert_eq!(report.decision.kind, GateDecisionKind::NeedToClarify);
        assert!(report.question.unwrap().question.contains("goal"));
    }

    #[test]
    fn repairs_thin_markdown_into_gate_dispatchable_contract() {
        let report = repair_markdown("Implement Forge", "make forge better");
        let validation = validate_markdown("Implement Forge", &report.repaired_markdown);

        assert!(report
            .repaired_markdown
            .contains("## Decisions / Assumptions"));
        assert!(report.repaired_markdown.contains("## Issue Goal"));
        assert!(validation.decision.is_dispatchable());
    }

    #[test]
    fn discover_returns_one_local_candidate_without_gsd_terms() {
        let candidates = discover_candidates("add issue forge validate and repair commands");

        assert_eq!(candidates.len(), 1);
        assert!(matches!(
            candidates[0].classification,
            CandidateClassification::ReadyWithAssumptions
        ));
        assert!(!candidates[0].title.to_lowercase().contains("gsd"));
    }
}
