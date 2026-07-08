# Workflow Activation

Status: Draft

## Purpose

Define when Symphony starts an `IssueWorkflow` execution.

Tracker is the durable queue and external workflow state. `IssueWorkflow` is
an executable orchestration episode, not a live execution for every
Shea-managed issue from `Backlog` through `Done`.

## Core Model

Use at most one active `IssueWorkflow` execution per issue at a time.

Between executable episodes, the tracker lane is the durable queue. Static
issues do not need live Workflow executions.

```text
Tracker lane transition creates executable condition
  -> Workflow Coordinator observes executable state
  -> Workflow Coordinator starts one IssueWorkflow execution for that issue
  -> Workflow performs work and commits next tracker state
  -> Workflow continues while executable work remains, otherwise completes
     and hands back to tracker/static lane
```

Workflow population is based on executable tracker states, not all
Shea-managed issues in the tracker.

## Identity

Use two identities:

- `workflow_id`: the human-readable Symphony execution identity and the Temporal
  Workflow ID;
- `run_id`: the Temporal-native execution locator returned by Temporal after
  start.

Recommended `workflow_id` shape:

```text
issue:<repo-slug>:<issue-number>:pulse:<from-state>-to-<target-kind>:<YYYYMMDD-HHMMSSZ>:<source-slug>
```

Examples:

```text
issue:shea-symphony:123:pulse:todo-to-work:20260708-134218Z:project-rev-456
issue:shea-symphony:123:pulse:human-review-to-merge:20260708-150012Z:operator-action-789
```

Use `target-kind` rather than guaranteed final state. A `Todo` pulse may end
in `In Progress`, `Need to Clarify`, `Agent Review`, or `Need Human Input`.
The Workflow ID should describe why the pulse started, not pretend to know its
final state.

SQLite, artifacts, logs, and App trace use `workflow_id` as the primary
semantic key. Add `run_id` when exact Temporal execution lookup is required.

The tradeoff is explicit: the complete issue lifecycle is not stored under one
Temporal Workflow ID. Full traceability comes from tracker state, SQLite
workflow index rows, artifacts, logs, Temporal Visibility/Search Attributes,
and the list of Workflow IDs for the issue.

## Static Lanes

These lanes are tracker queues by default and do not automatically keep an
active `IssueWorkflow` execution open:

- `Backlog`;
- `Human Review`;
- `Need Human Input`, unless automatic doctor/reconcile work is required;
- `Done`.

Static does not mean unmanaged. It means the tracker is holding the durable
state until an executable condition appears.

## Executable States

These states can start an `IssueWorkflow` execution or find an existing active
execution:

- `Todo`: contract check and implementation entry;
- `In Progress`: Main agent work;
- `Agent Review`: agent review work;
- `Rework`: rework agent work;
- `Merging`: land/merge flow;
- `Need Human Input`: only when automatic doctor/reconcile work is required.

`Backlog` promotion to `Todo` creates an executable condition. `Todo` is the
workflow activation point.

## Human Review And Operator Actions

`Human Review` is a static human lane by default.

When a human approves, requests rework, or applies a small fix, the App routes
to Codex/operator flow. The routed flow submits a structured action through the
Operator Action Bridge. Symphony validates the action and starts the
appropriate executable episode:

- approval or human fix can activate validation and `Merging`;
- requested changes can activate `Rework`;
- unresolved ambiguity can move to `Need Human Input`.

The App does not keep a live Workflow open merely because an issue is waiting
for a human.

## Workflow Coordinator

The Workflow Coordinator is a thin launcher and registrar. It should:

- read tracker states;
- identify executable states;
- construct the human-readable `workflow_id`;
- check the SQLite active workflow index and Temporal visibility when needed;
- start `IssueWorkflow` executions only for executable states;
- record `workflow_id` and Temporal `run_id`;
- enforce at most one active execution per issue at a time;
- respect task queue and Activity concurrency limits;
- ignore static issues except for dashboard/read-model projection.

It must not run agents, choose business transitions, or directly write tracker
state. The Workflow owns orchestration decisions. Activities own side effects.

Task queue concurrency controls active work, not the number of static issues in
the tracker.

## Tracker Validation

Do not make every Workflow step read the tracker. Tracker reads should happen
at durable boundaries:

- Workflow start reads current tracker state and source revision;
- long-running agent work may perform one start-boundary validation;
- tracker transition Activities validate expected state before writing;
- fact-changing writes perform targeted readback after writing;
- external tracker changes become typed conflicts or human-input cases.

Normal in-flight decisions rely on Temporal state, Activity results, SQLite
active workflow index rows, and artifact references. Manual tracker edits are
an exception path; they should not make the normal path pay for repeated full
tracker reads.

## Episode Completion

An `IssueWorkflow` episode should complete after it commits a static tracker
state and required read-model projection. It may continue only while the issue
remains in executable states.

Examples:

- `Todo` contract check passes and starts implementation, continuing into the
  executable work episode;
- `In Progress` completes PR/link/handoff work and commits `Agent Review`;
- `Agent Review` passes and commits `Human Review`, then completes because
  `Human Review` is static;
- `Merging` lands work and commits `Done`, then completes;
- an operational blocker commits `Need Human Input`, then completes unless an
  automatic doctor/reconcile episode is required.

## Non-Goals

- No live Workflow execution for every tracker issue.
- No long-lived idle Workflow for `Backlog`.
- No long-lived idle Workflow for `Human Review`.
- No App-side scheduler.
- No custom autopilot queue beside tracker plus Temporal.
