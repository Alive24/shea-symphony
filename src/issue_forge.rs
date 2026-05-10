use serde::{Deserialize, Serialize};

use crate::model::{GateDecision, GateDecisionKind};

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
}
