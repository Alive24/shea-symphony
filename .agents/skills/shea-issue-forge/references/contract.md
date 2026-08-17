# Executable Issue Contract

Use the repository's Issue Quality Gate and draft this shape, omitting only
genuinely inapplicable optional reference sections:

```markdown
## Issue Setup
- UAT Required: Yes / No
- Assignee: <resolved default>
- Dependencies: None / <structured blocker>
- Related Parent Issue or Context: <link or None>

## Issue Goal
<one concrete outcome>

## Issue Context
### Why Now
<why now>
### Target Repository / Package
- <resolved target>

## Non-Negotiable Guardrails
- <safety boundary>

## Scope
### In Scope
- <deliverable>
### Out of Scope
- <exclusion>

## Canonical References
### Relevant Knowledge Sources
- <local path or external URL>
### Relevant Code Paths
- <path>

## Current State
<what was checked and when>
### Code-State Freshness
<base, relevant PRs, and known drift>

## Deliverable Shape
<observable result>

## Risks or Constraints
- <risk or None>

## Expected Outcome
- [ ] <objective result>

## Verification
### Completion Criteria
- [ ] <objective condition>
### Functional Verification
- [ ] <repository-supported command or test>
### UAT
- [ ] <operator action, or Not required>
### Context Verification
- [ ] Confirm the contract still matches current base, relevant PRs, and recent work before dispatch.
```

Every checklist item must be independently checkable from a diff, workpad,
timeline evidence, command result, or operator evidence. Keep local paths plain
so the quality gate can resolve them.
