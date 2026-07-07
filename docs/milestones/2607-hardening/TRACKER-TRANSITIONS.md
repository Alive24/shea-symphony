# Tracker Transitions

Status: Draft

## Purpose

Define the hard boundary for tracker state changes.

2607 Hardening should introduce a Symphony-owned transition path that all
lanes, current workflow steps, App commands, Shea extensions, and future
Workflow Graph nodes use when tracker state changes.

## Principle

Separate three things:

- proposal: a lane, extension node, LLM, or operator proposes a next state;
- decision: Symphony validates whether that proposal is allowed now;
- commit: Symphony writes tracker state, runtime state, and evidence.

Extension nodes may influence workflow direction by selecting graph edges or
proposing the next core node. They do not directly commit tracker state.

This keeps the workflow agentic without letting extension logic bypass the
runtime boundary.

## API Shape

Use an internal API as the primary surface. The CLI should be a thin wrapper.

```text
SymphonyTransitionService.requestTransition(...)

symphony transition --issue <id> --to <state> --reason <reason>
```

Internal runtime code and CLI commands must share the same validation, evidence,
tracker write, runtime write, and reconcile behavior.

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

The App may trigger display-oriented CLI commands:

- read snapshot;
- refresh tracker cache;
- controlled tick/pause/autopilot commands.

The App must not directly call tracker mutation or bypass transition checks.

## First Implementation Bias

Start by documenting and testing the transition table before moving every call
site.

First useful refactor target:

- introduce the transition service shape;
- route one low-risk state move through it;
- record evidence consistently;
- keep old behavior working until migration is complete.
