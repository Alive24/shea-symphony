# Jade Workflow

Status: Bootstrap v0

This file describes the Jade-specific workflow policy. It should be used after
reading the official reference workflow at:

`docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`

## Workflow Intent

Jade Symphony keeps the official Symphony shape:

- read tracker work.
- create isolated workspace.
- render issue prompt.
- run coding agent.
- update tracker/workpad.
- reconcile state.

Jade-specific additions are:

- GitHub Project v2 as the first tracker.
- assignee filtering for multiple owners.
- Issue Forge for proposing, drafting, validating, and repairing executable
  issues.
- question-driven clarification loops before issue dispatch when intent is
  underspecified.
- issue quality gate before dispatch.
- extra normalized states.
- independent `Agent Review` before `Human Review`.
- Codex and Claude Code as peer agent backends.

## Status Map

- `Backlog`: out of scope; do not modify.
- `Todo`: queued; run issue quality gate before implementation.
- `Need to Clarify`: issue contract failed quality gate; wait for human
  clarification.
- `In Progress`: implementation actively underway.
- `Need Human Input`: implementation started but continuation needs product
  decision, missing external information, credentials, sample data, or
  confirmation.
- `Agent Review`: implementation is complete enough for an independent agent
  review.
- `Human Review`: PR is attached, local validation passed, and agent review is
  resolved.
- `Rework`: review requested changes.
- `Merging`: approved by human; run land flow.
- `Done`: terminal state.

## Issue Work Cycle

Every executable issue should move through these stages:

1. `Context`: read issue, canonical docs, code, recent progress, linked PRs, and
   workpad.
2. `Decision capture`: identify gray areas and record decisions or assumptions.
3. `Execution plan`: write a small checklist with target surfaces and
   validation.
4. `Build`: implement only the accepted scope in the isolated workspace.
5. `Verify and repair`: run required validation, repair gaps, and rerun.
6. `Agent review`: request independent review and resolve or reject findings.
7. `Handoff`: create PR, update workpad, transition to `Human Review`.

## Issue Forge Cycle

Issue Forge operates before normal issue execution. It is used when a human
intent, roadmap gap, technical document, recent implementation change, or
existing underspecified issue needs to become an executable tracker issue.

Issue Forge should move through these stages:

1. `Source scan`: read the human prompt, existing tracker work, canonical docs,
   recent progress, and relevant code reality.
2. `Candidate framing`: propose one or more issue candidates and classify each
   as `Ready`, `Ready With Assumptions`, `Need to Clarify`, `Too Broad`,
   `Blocked`, or `Duplicate / Already Covered`.
3. `Clarification loop`: for a selected candidate, ask one focused question at
   a time until the answer no longer materially changes scope, acceptance
   criteria, guardrails, references, or validation.
4. `Decision capture`: write human answers into the issue draft as decisions,
   assumptions, non-goals, or follow-up candidates.
5. `Issue contract draft`: produce the issue using
   `docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md`.
6. `Gate check`: validate the draft against the Issue Quality Gate before
   creating or updating the tracker issue.

Clarification loop rules:

- Ask the smallest useful question that unblocks executability.
- Do not interrogate the human for details that can be discovered from local
  docs, tracker state, or code.
- Do not continue asking after the issue can pass the quality gate.
- If a new branch of work appears, record it as a follow-up candidate rather
  than expanding the current issue.
- If a missing answer is execution-critical, classify the candidate or issue as
  `Need to Clarify`.
- If a missing answer is non-critical, record it as an assumption and continue.

## Issue Quality Gate

Before moving from `Todo` to `In Progress`, check whether the issue contract is
executable.

If the issue is missing critical context:

1. Keep code unchanged.
2. Create or update the workpad with the missing context.
3. Move status to `Need to Clarify`.
4. Ask for the smallest useful clarification.

If the issue is executable with assumptions:

1. Record assumptions in the workpad.
2. Move status to `In Progress`.
3. Continue implementation.

## Agent Review Gate

Before `Human Review`, route completed implementation to `Agent Review`.

Minimum behavior:

1. Run an independent reviewer backend or reviewer prompt.
2. Classify findings as `Confirmed`, `Plausible`, `Rejected`, or `Needs Context`.
3. Fix confirmed issues before human handoff.
4. Record rejected or deferred findings in the workpad.
5. Move to `Human Review` only after validation and review are resolved.

## Workpad Outline

```markdown
<!-- jade-symphony-workpad -->
## Jade Symphony Workpad

Environment: `<host>:<workspace>@<short-sha>`

### Context
- [ ] ...

### Decisions / Assumptions
- ...

### Plan
- [ ] ...

### Validation
- [ ] ...

### Agent Review
- ...

### Handoff
- ...
```
