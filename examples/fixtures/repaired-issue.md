## Issue Setup

- UAT Required: No
- Related Parent Issue or Context: Local Issue Forge smoke fixture.

## Issue Goal

Make Issue Forge validate rough ideas and repair them into executable Jade Symphony issue contracts.

## Why Now

Operators need a local way to turn rough issue text into a dispatchable contract before live tracker mutation exists.

## Issue Context

Source input captured by Issue Forge:

```md
Make Issue Forge validate rough ideas and repair them into executable Jade Symphony issue contracts.
```

## Decisions / Assumptions

### Decisions

- Use Issue Forge repair to convert rough input into the Jade Symphony quality template.

### Assumptions

- Local Markdown repair is sufficient for the first operator workflow.

## Non-Negotiable Guardrails

- Keep Jade Symphony orchestration infrastructure separate from downstream product business logic.
- Do not introduce GSD runtime naming.

## Scope

### In Scope

- Local Forge validation and repair smoke testing.

### Out of Scope

- Live tracker issue creation.

## Canonical References

### Target Repository / Package

- Alive24/jade-symphony

### Relevant Knowledge Sources

- docs/bootstrap/JADE_SYMPHONY_SPEC.md
- docs/bootstrap/JADE_WORKFLOW.md
- docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md

### Relevant Code Paths

- src/issue_forge.rs
- src/main.rs

## Current State

Rough issue input exists and has been repaired into the Jade Symphony issue contract shape.

## Deliverable Shape

CLI output and validated issue-contract Markdown.

## Risks or Constraints

- Do not expand scope beyond the repaired issue contract without creating follow-up candidates.

## Expected Outcome

An executable issue contract that can pass the Issue Quality Gate before dispatch.

## Verification

### Completion Criteria

- Issue contract has goal, scope, guardrails, references, and validation requirements.

### Functional Verification

- Run `jade-symphony forge validate` on the repaired draft.

### UAT

- Not required for this fixture.

### Context Verification

- Confirm the issue still matches canonical sources.
