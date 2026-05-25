# Shea Symphony Workflow

Status: Bootstrap v0

This file describes the Shea Symphony-specific workflow policy. It should be used after
reading the official reference workflow at:

`docs/bootstrap/references/openai-symphony/elixir/WORKFLOW.md`

## Workflow Intent

Shea Symphony keeps the official Symphony shape:

- read tracker work.
- create isolated workspace.
- render issue prompt.
- run coding agent.
- update tracker/workpad.
- reconcile state.

Shea Symphony-specific additions are:

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

## Autonomous Operating Loop

When asked to continue work, Shea Symphony should keep advancing the tracker
until it reaches a stop condition. It should not stop after completing one issue
merely because the current prompt named that issue.

Loop:

1. Refresh tracker state and worktree state.
2. Finish any active `In Progress` issue that is not blocked.
3. If no issue is active, pick the next executable `Todo` issue by priority and
   tracker ordering.
4. Run the Issue Quality Gate before implementation.
5. Execute the Issue Work Cycle.
6. Move main-agent completed work to `Agent Review`.
7. Return to step 1 and continue.

Stop only when one of these is true:

- no executable `Todo`, `Rework`, or active `In Progress` issue remains.
- the current issue must move to `Need to Clarify`.
- the current issue must move to `Need Human Input`.
- required credentials, local tools, sample data, or external services are
  missing and no safe fallback exists.
- verification fails and the failure cannot be repaired locally after diagnosis.
- continuing would require destructive action or a scope change not authorized
  by the issue contract.
- the human explicitly tells the agent to stop after a specific issue.

If a prompt says "do not continue to issue X until issue Y is complete", treat
that as a dependency constraint, not a permanent stop instruction. Once issue Y
is complete or handed off to the correct review state, resume the loop and
select the next executable issue.

## Status Map

- `Backlog`: out of scope; do not modify.
- `Todo`: queued; run issue quality gate before implementation.
- `Need to Clarify`: issue contract failed quality gate; wait for human
  clarification.
- `In Progress`: implementation actively underway.
- `Need Human Input`: implementation started but continuation needs product
  decision, missing external information, credentials, sample data, or
  confirmation.
- `Agent Review`: main-agent implementation is locally complete and awaiting
  independent Review Agent execution.
- `Human Review`: independent Review Agent has passed the work with recorded
  evidence; waiting on human approval.
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
6. `Agent review handoff`: main implementation agent updates the workpad and
   moves the issue to `Agent Review`.
7. `Independent review`: Review Agent runs asynchronously, records evidence,
   and either moves the issue to `Human Review`, moves it to `Rework`, or keeps
   it blocked in `Agent Review` / `Need Human Input`.

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

1. Main implementation agent validates local completion and moves the issue to
   `Agent Review`.
2. Main implementation agent must never set `Human Review`.
3. Independent Review Agent starts an asynchronous review job using the
   configured backend.
4. Review Agent records queued, running, completed, failed, timed out, and
   cancelled job state in a standalone append-only Agent Review timeline
   comment and the status surface.
5. Review Agent classifies findings as `Confirmed`, `Plausible`, `Rejected`, or
   `Needs Context`.
6. Review Agent moves confirmed findings to `Rework`.
7. Review Agent moves failed, timed out, inconclusive, or backend-unavailable
   reviews to `Need Human Input` or keeps them in `Agent Review` with explicit
   evidence.
8. Review Agent evaluates the issue body checkboxes under `Expected Outcome`,
   `Completion Criteria`, `Functional Verification`, `UAT`, and
   `Context Verification`, then checks only the items supported by PR/workpad
   evidence.
9. Review Agent routes only after review passes and evidence is recorded:
   ordinary issues and parent final issues move to `Human Review`, while
   routine native subissues move to `Merging` because the parent issue owns
   final Human Review and UAT.

## Evidence Timeline Model

Main implementation owns one persistent `Main Agent Workpad` comment. It records
current implementation context, plan, work log, changed files, verification, PR
handoff, and Main-lane Rework implementation rounds.

Other lanes do not edit or restructure that Workpad. Review attempts, Rework
trigger diagnostics, Merge runs, Human Review decisions, and Doctor triage or
repair records each write their own append-only issue timeline comment. Each
timeline comment should be self-contained enough to audit in chronological
order: human-readable timestamp with timezone, run id, lane, actor, input state,
target state, PR when relevant, result, and evidence summary.

## Workpad Outline

```markdown
<!-- shea-symphony-workpad -->
## Shea Symphony Workpad

Environment: `<host>:<workspace>@<short-sha>`

### Context
- [ ] ...

### Decisions / Assumptions
- ...

### Plan
- [ ] ...

### Work Log
- `<timestamp>` ...

### Validation
- [ ] ...

### Agent Review
- ...

### Handoff
- ...
```
