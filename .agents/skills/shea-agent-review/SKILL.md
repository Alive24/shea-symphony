---
name: shea-agent-review
description: Trigger one independent Shea Symphony Agent Review for a named ready issue through the external Review backend selected by the active workflow, then read back its recorded decision and routing.
metadata:
  short-description: Run one independent Shea review
---

# Shea Symphony Agent Review

This skill launches one external Review backend. Do not review the code yourself, manufacture evidence, or blur Review into implementation.

## Resolve and inspect

Read `.shea/contracts/workflow-capability.v1.md`, resolve its active workflow, and select a supported adapter. The workflow owns tracker, workspace, branch, Review backend, timeout, and state policy; the adapter owns command syntax.

Use targeted `issue.read`, `issue.inspect`, `evidence.read`, `pull_request.read`, and `relationships.read`. Require Agent Review state, one ready non-draft linked PR, a consistent canonical workspace/Main handoff, no conflicting Review claim/job, and an available external backend. Review independence, linked-PR evidence, and workspace identity fail closed.

Routine native subissue PASS routes to `Merging`, not `Human Review`; the parent owns final Human Review/UAT unless an explicit exception is recorded.

## Invocation authorization

An operator invocation bound to exactly one operator-selected issue—either named in the invocation or uniquely established by the operator in the current task—is explicit confirmation for one wrapper-owned Agent Review run using the backend resolved from the active workflow. It authorizes the wrapper to:

- send only the selected issue contract, linked ready PR and diff, canonical workspace repository content, and required Main handoff/evidence to that backend;
- allow workflow-authorized read-only repository inspection plus wrapper-owned local logs, ledger, scratch, and localhost language-server access;
- write the Review claim, append Review evidence, update supported non-UAT checklist evidence, and make the decision-derived Review state transition last.

An issue discovered only through queue scanning, dry-run, or preflight is not operator-selected; show the prepared effect and obtain confirmation before launching it. Otherwise, after preflight proves the exact issue, PR, workspace, backend, and bounded actions, launch without asking the operator for the same authorization again. Require new explicit authorization if any of those resolved identities changes after preflight, the backend requests tools or write authority beyond configured read-only Review, content would cross the selected repository/PR boundary, or standalone preparation is required. This invocation does not authorize workflow/backend changes, implementation, Human approval, or Merge.

## Launch and read back

Use the adapter's single targeted Review operation. Do not manually write Review claim fields, pass/reject results, output files, or Project state around it. Let the wrapper claim, render the configured Review prompt, run the external backend, append the standalone `Shea Symphony Agent Review Run`, update supported non-UAT checklist evidence, and make state the final mutation.

After completion, use targeted readback only. Report the backend/run, PR, recorded evidence, and resulting state. PASS may route to Human Review (ordinary/parent) or Merging (routine child); confirmed defects route to Rework; unavailable, timed-out, draft, ambiguous, or inconclusive runs must not reach Human Review.

## Explicit standalone fast path

Only when the operator explicitly authorizes reviewing a standalone implementation, prepare one unambiguous ready PR/workspace/link, record the exception as append-only evidence, and make Agent Review the final preparation mutation before launching. Do not invent a Main claim or automated Main workpad.

For normal operations, prefer the operator-controlled `autopilot plan` / `autopilot loop` foreground workflow; this skill remains the one-issue launcher.
