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

## Start Contract

Coordinator start is optimistic with a local guard:

```text
read tracker executable state
  -> derive workflow_id
  -> insert workflow_index row with status=starting
  -> start Temporal Workflow
  -> store run_id and set status=running
```

The SQLite row makes startup observable and reduces duplicate starts. Temporal
start success is the execution fact. SQLite is not the authoritative workflow
runtime.

Start conflicts:

- SQLite insert conflict: inspect the active row and repair it if stale;
- Temporal open Workflow already exists: bind to the existing execution and
  repair `workflow_index`;
- Temporal closed Workflow with the same `workflow_id`: generate a new
  `workflow_id` with a new timestamp/source or attempt suffix.

## Repair Contract

The Coordinator owns local repair of `workflow_index`. It does not repair
business state by directly moving tracker lanes.

Repair matrix:

| Local State | Temporal State | Action |
| --- | --- | --- |
| `starting` without `run_id` | not started or unknown | mark `stale_start` after timeout; reread tracker before retry |
| missing row | active execution exists | rebuild projection from Temporal visibility/query; do not start another execution |
| active row | active execution exists | keep row, refresh progress/freshness |
| active row | closed execution exists | mark `completed` or `failed` when close status is known; otherwise `closed_unknown` |
| active row | execution not found | mark `stale_missing`; reread tracker before retry |
| stale row | tracker still executable | generate a new `workflow_id` and start a new execution |
| stale row | tracker is static | leave closed/stale projection; do not start |

Use conservative stale thresholds:

- `starting` without `run_id` for more than a short startup timeout;
- `running` without heartbeat/progress beyond its configured TTL;
- Temporal visibility/query cannot confirm the recorded active execution.

Repair triggers in 2607:

- App start performs one repair pass;
- refresh/snapshot may run lightweight repair for visible issues;
- Coordinator start performs targeted repair for the target issue.

Do not introduce a background Symphony daemon or full-time scanner in 2607.

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

An `IssueWorkflow` execution completes only at a real terminal boundary:

- `completed_static_handoff`: tracker was committed to a static lane such as
  `Human Review`, normal `Need Human Input`, or `Backlog`;
- `completed_done`: tracker was committed to `Done`;
- `failed_unhandled_error`: the Workflow cannot safely converge;
- `cancelled`: operator or system policy cancelled the execution.

When the next state is still executable, the Workflow may continue to the next
lane handler in the same execution. That continuation is not a terminal
outcome and should not be exposed as a completed status.

Executable lane handlers are independently startable and chainable. Coordinator
can start from `Todo`, `In Progress`, `Agent Review`, `Rework`, `Merging`, or
automatic doctor/reconcile work in `Need Human Input`. If one handler produces
another executable state, continuing in the same execution is allowed and can
reduce handoff overhead.

Examples:

- `Todo` contract check passes and starts implementation, continuing into the
  `In Progress` handler;
- `In Progress` completes PR/link/handoff work and commits `Agent Review`;
- `Agent Review` passes and commits `Human Review`, then completes because
  `Human Review` is static with `completed_static_handoff`;
- `Merging` lands work and commits `Done`, then completes;
- an operational blocker commits `Need Human Input`, then completes unless an
  automatic doctor/reconcile episode is required.

## Non-Goals

- No live Workflow execution for every tracker issue.
- No long-lived idle Workflow for `Backlog`.
- No long-lived idle Workflow for `Human Review`.
- No App-side scheduler.
- No custom autopilot queue beside tracker plus Temporal.
