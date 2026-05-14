# Issue Quality Gate Template

Status: Bootstrap v0

Use this as the preferred issue contract shape for work that Jade Symphony may
dispatch to coding agents. The goal is not bureaucracy; the goal is to make the
issue executable without forcing the agent to invent product, architecture, or
operational intent.

```md
## Issue Setup

- UAT Required: Yes / No
- Related Parent Issue or Context: [optional GitHub issue, Linear issue, or document]

## Issue Goal

[Describe the concrete outcome this issue is meant to achieve. This should read as the main execution target.]

## Why Now

[Explain why this issue should be handled now: blocker, sequencing dependency, risk reduction, roadmap priority, regression, operational need, or newly discovered gap.]

## Issue Context

[Describe the operating context for this issue: what already exists, what environment or architecture this issue sits within, what prior assumptions matter, and what execution reality the coding agent needs to understand before planning.]

This section should include enough embedded context that the assignee can start without opening a long chain of supporting artifacts.

If needed, split the context into subsections such as:

### Architectural Context

- ...
- ...

### Product / Design Context

- ...
- ...

### Business / Operational Context

- ...
- ...

## Decisions / Assumptions

Use this section to preserve answers from the Issue Forge clarification loop.

### Decisions

- ...
- ...

### Assumptions

- ...
- ...

## Dependencies

Every executable issue must say whether it has blocking dependencies. Use this
section to make semantic dependency status explicit before dispatch.

- No blocking dependencies.
- Blocked By: [issue/PR/decision/data dependency and required terminal state]
- Related / Overlapping Issues: [issue links and why they are not blockers, or why this issue should not run independently]
- Parallel-Safe With: [issues that may run concurrently without invalidating this work]

## Non-Negotiable Guardrails

- ...
- ...
- ...

## Scope

### In Scope

- ...
- ...

### Out of Scope

- ...
- ...

## Canonical References

Use this section to point at the minimum set of sources the assignee and coding agent should trust first.

### Target Repository / Package

- `owner/repo` or local repo path
- package/module/workspace if relevant

### Relevant Knowledge Sources

- `docs/...`
- `knowledge/...`
- `ADRs/...`
- [GitHub issue or document](https://github.com/...)
- [Linear document or issue](https://linear.app/...)
- `.planning/...`

### Relevant Code Paths

- `path/to/file-or-directory`
- `path/to/file-or-directory`

## Current State

[Short summary of the current implementation or planning state.]

## Deliverable Shape

[State what form the result should take: docs, code, anchor files, planning artifacts, refactor, issue update, or validated behavior.]

## Risks or Constraints

- ...
- ...

## Expected Outcome

[State what should exist when this issue is complete: artifact, implementation state, decision, or validated result.]

## Verification

### Completion Criteria

- ...
- ...

### Functional Verification

- ...
- ...

### UAT

- If `UAT Required` is `Yes`, run the configured human-facing UAT workflow before treating the issue as fully closed.
- Use UAT only for issues with user-observable or operator-observable outcomes.

### Context Verification

- Confirm that the issue still matches the latest relevant canonical sources.
- Confirm that any new durable knowledge has been promoted to the relevant canonical document if needed.
```

## Gate Decision

Before dispatch, classify the issue as one of:

- `Ready`: executable without major assumptions.
- `Ready With Assumptions`: executable, but assumptions must be recorded in the workpad before implementation.
- `Need to Clarify`: missing critical execution context; ask for the smallest useful clarification.
- `Too Broad`: should be split before dispatch.
- `Blocked`: external dependency, credentials, sample data, or decision is missing.
- `Duplicate / Already Covered`: do not dispatch; link the canonical issue or artifact.

Dependency semantics are part of the gate. If an issue omits this section or
uses placeholder language such as `TBD`, `unknown`, or "potential dependency",
route it to `Need to Clarify`. If tracker-level blockers are present, the main
agent must not claim the issue until every blocker is terminal.
