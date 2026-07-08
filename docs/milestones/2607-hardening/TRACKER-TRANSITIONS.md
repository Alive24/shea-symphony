# Tracker Transitions

Status: Draft

## Purpose

Define the hard boundary for tracker state changes.

2607 Hardening should move tracker state changes behind
`TrackerTransitionActivity`. `IssueWorkflow` requests transitions; the Activity
validates, writes tracker state, writes evidence, and returns the committed
summary.

## Principle

Separate three things:

- proposal: workflow code, an Activity result, extension output, or operator
  signal proposes a next state;
- decision: `IssueWorkflow` decides which transition to request;
- commit: `TrackerTransitionActivity` writes tracker state and evidence.

Extension nodes may influence workflow direction by selecting graph edges or
proposing the next core node. They do not directly commit tracker state.

This keeps the workflow agentic without letting extension logic bypass the
runtime boundary.

## Activity Shape

Use a Temporal Activity as the primary write surface.

```text
TrackerTransitionActivity(request) -> committed_transition
```

App, CLI, lanes, and extensions must not bypass this Activity.

The detailed Activity contract is recorded in
`TRACKER-TRANSITION-ACTIVITY.md`.

## Standard States

- `Backlog`
- `Todo`
- `Need to Clarify`
- `In Progress`
- `Need Human Input`
- `Agent Review`
- `Human Review`
- `Merging`
- `Rework`
- `Done`

## Required Evidence

Every committed transition should write evidence unless the operation is a pure
read or cache refresh.

Minimum fields:

- issue id;
- from state;
- to state;
- requested by;
- committed by;
- reason;
- workflow step id, with optional future graph node id;
- trace id;
- artifact references;
- timestamp.

Evidence is required so an operator can answer why a state changed, why it got
stuck, and what can be retried.

## Extension Authority

Extensions may:

- inspect context;
- call LLMs under policy;
- produce evidence;
- choose or recommend graph edges;
- request transitions;
- request entry into a standard core node.

Extensions may not:

- write tracker fields directly;
- close issues directly;
- clear claims directly;
- merge PRs directly;
- bypass required validation;
- mark terminal completion directly.

If an extension node is inserted immediately before a core node, it may decide
which core node should run next. Symphony still validates and commits any
tracker transition needed to enter that node.

## Restricted Commits

These transitions or side effects should only be committed by Symphony-owned
lane logic:

- `Merging` to `Done`;
- issue closure;
- claim cleanup;
- PR merge;
- PR creation or binding;
- terminal result writes;
- destructive worktree operations.

Extensions may request these outcomes, but the commit belongs to Symphony.

## Handoff Completion

A lane handoff is complete only after the tracker transition succeeds.

If code work finishes but tracker transition fails:

- local runtime records the worker result;
- handoff status becomes transition failed or reconcile needed;
- the lane does not claim completion;
- Symphony retries or enters a reconcile path.

Local completion without tracker transition success is evidence, not workflow
completion.

## Claim Ownership

Use a two-layer claim model.

Tracker-visible claim:

- active lane;
- claim owner;
- claimed at;
- human-readable status.

Local runtime claim:

- worker session;
- attempt id;
- heartbeat;
- worktree;
- last progress timestamp;
- local process or Codex thread reference.

The tracker should show who owns the workflow item. Local runtime state should
hold detailed execution machinery.

2607 should reduce tracker field flags where they only exist for local recovery
or duplicate-claim prevention. Those details belong in Temporal workflow state,
Activity heartbeat/progress, local artifacts, and idempotent recovery markers.
GitHub Project fields should be reserved for human-visible state and coarse
ownership facts.

## Need States

Use enum reasons plus freeform detail.

`Need to Clarify` reason examples:

- `missing_contract`;
- `ambiguous_scope`;
- `insufficient_acceptance_criteria`;
- `conflicting_requirements`.

`Need Human Input` reason examples:

- `requires_secret`;
- `requires_external_account`;
- `dangerous_operation`;
- `external_service_failure`;
- `manual_decision_required`;
- `local_environment_blocked`;
- `tracker_state_conflict`.
- `merge_semantic_fix_failed`;

Enums support filtering, dashboard display, and automation. Freeform detail
keeps the state useful to humans.

## Human Review To Merging

`Human Review` may move to `Rework`, or a human may make a small fix and move
the item to `Merging`.

Before `Merging`, Symphony should run lightweight validation:

- PR still exists;
- branch is current enough for the configured policy;
- required checks pass or are explicitly accepted;
- diff since last agent review is summarized;
- human modification is acknowledged;
- required review comments are resolved or explicitly deferred.

If the human change is materially risky, the graph can route back to
`Agent Review` or `Rework`.

## Merging Semantic Fix

`Merging` may perform semantic fixes in place through `MergeActivity` or a
dedicated `MergeSemanticFixActivity`.

If the semantic fix succeeds, continue merging. If it cannot resolve the
problem, move to `Need Human Input`. Do not bounce merge-time semantic fix
failure to `Rework` by default; the merging coding agent is not a meaningful
handoff back to Main.

## External Tracker Changes

Tracker state is the external workflow fact. Runtime state is local execution
evidence.

When Symphony sees tracker state changed outside its transition path:

- record a reconcile event;
- accept the tracker as the external fact;
- if no active local conflict exists, continue from the new state;
- if active runtime state conflicts, pause and move to `Need Human Input` with
  reason `tracker_state_conflict`.

Do not silently continue through a conflicting external state change.

## App Boundary

The App may call Tauri backend commands that perform Temporal operations:

- workflow query;
- workflow signal;
- workflow update;
- workflow start.

The App must not directly call tracker mutation or bypass
`TrackerTransitionActivity`.

## Complete Migration Bias

The 2607 target is complete tracker transition ownership, not a narrow first
batch. Break the work into submilestones so implementation is reviewable, but
do not treat unchecked transition paths as acceptable final scope.
