# T2607-03 Workflow Coordinator

Status: Draft

## Purpose

Implement the thin launcher and registrar that turns executable tracker states
into Temporal `IssueWorkflow` executions.

The Coordinator is not a scheduler, not an agent runner, and not a workflow
decision engine. It is the boundary that answers:

- should this tracker issue have an active workflow execution now?
- if yes, what is the deterministic human-readable `workflow_id`?
- did Temporal actually start or already have the execution?
- did the local SQLite `workflow_index` record the active execution correctly?

## Inputs

This package implements decisions from:

- `WORKFLOW-ACTIVATION.md`;
- `TEMPORAL-CONCURRENCY.md`;
- `TASK-QUEUES.md`;
- `LOCAL-STATE-DB.md`;
- `adr/0006-temporal-local-runtime-spine.md`.

## Goals

- Start `IssueWorkflow` only for executable tracker states.
- Enforce at most one active `IssueWorkflow` execution per issue at a time.
- Use a human-readable episode-scoped `workflow_id` as the Temporal Workflow ID.
- Store Temporal's native `run_id` after start for exact execution lookup.
- Use SQLite `workflow_index` as the local active guard and App read model.
- Use Temporal start/visibility/query as the execution fact.
- Repair stale local rows without directly changing tracker business state.
- Keep startup and refresh cheap enough for App use.

## Non-Goals

- No background Symphony daemon.
- No App-owned scheduler.
- No full-time tracker scanner.
- No agent execution inside the Coordinator.
- No direct tracker state write inside the Coordinator.
- No business decision about whether an implementation, review, rework, or
  merge should pass.
- No replacement for `IssueWorkflow` state handling.
- No replacement for `TrackerTransitionActivity`.

## Expected Code Areas

Recommended package shape:

```text
symphony/
  coordinator/
    mod.rs
    start.rs
    repair.rs
    discovery.rs
    identity.rs
    capacity.rs
    dto.rs
```

Names are illustrative. Keep the implementation inside the `symphony` runtime
boundary unless the existing codebase strongly suggests a better local module
layout.

## Core DTOs

Recommended DTOs:

```text
CoordinatorStartRequest {
  issue_ref
  source
  expected_tracker_state?
  target_kind?
  reason
  force_repair_before_start: bool
}

CoordinatorStartResult {
  issue_ref
  action
  workflow_id?
  run_id?
  tracker_state
  local_status
  temporal_status?
  freshness
  message
}

CoordinatorRepairRequest {
  issue_ref
  scope
  reason
}

CoordinatorRepairResult {
  issue_ref
  before
  after
  action
  message
}
```

Initial `CoordinatorStartResult.action` enum:

- `started`;
- `already_running`;
- `static_state`;
- `capacity_deferred`;
- `repaired_and_started`;
- `repair_required`;
- `conflict`;
- `failed`.

Initial repair action enum:

- `no_op`;
- `marked_stale_start`;
- `marked_stale_missing`;
- `marked_completed`;
- `marked_failed`;
- `marked_closed_unknown`;
- `bound_existing_temporal_execution`;
- `cleared_inactive_guard`;
- `start_retry_allowed`;
- `failed`.

Use typed `RepoId`, `IssueRef`, `WorkflowId`, and `RunId` wrappers at code
boundaries. Plain strings are acceptable only inside persistence and Temporal
client calls.

## Executable State Policy

The Coordinator should classify tracker states before attempting a start.

Executable states:

- `Todo`;
- `In Progress`;
- `Agent Review`;
- `Rework`;
- `Merging`;
- `Need Human Input` only when automatic doctor/reconcile work is explicitly
  requested.

Static states:

- `Backlog`;
- `Need to Clarify`;
- normal `Need Human Input`;
- `Human Review`;
- `Done`.

`Backlog` promotion is a tracker/operator action that may create `Todo`.
`Todo` is the executable activation point for `IssueWorkflow`.

## Workflow ID Construction

The Coordinator owns `workflow_id` construction.

Recommended shape:

```text
issue:<repo-slug>:<issue-number>:pulse:<from-state>-to-<target-kind>:<YYYYMMDD-HHMMSSZ>:<source-slug>
```

Rules:

- `workflow_id` is the Temporal Workflow ID.
- Store Temporal's returned `run_id` separately.
- Use `target-kind`, not promised final state.
- Include a human-readable UTC timestamp.
- Include a source slug that explains why the pulse started.
- If the same issue needs a later execution, generate a new `workflow_id`.

Examples:

```text
issue:shea-symphony:123:pulse:todo-to-work:20260708-134218Z:project-rev-456
issue:shea-symphony:123:pulse:human-review-to-merge:20260708-150012Z:operator-action-789
```

The source slug should be stable enough for audit but not contain secrets or
large payloads. Good source examples:

- `project-rev-456`;
- `operator-action-789`;
- `app-start-repair`;
- `visible-refresh`;
- `doctor-request`.

## Start Flow

Normal start:

```text
read targeted tracker issue state
  -> classify executable/static
  -> check local active workflow guard
  -> perform targeted repair if the guard may be stale
  -> check configured capacity
  -> construct workflow_id
  -> insert workflow_index row with status=starting
  -> start Temporal IssueWorkflow on symphony-core
  -> capture run_id
  -> update workflow_index status=running
  -> return started
```

If the tracker state is static, return `static_state` and do not create a
Workflow execution.

If capacity is exhausted, return `capacity_deferred`. Do not create a local
`starting` row before capacity is available.

If SQLite insert fails because an active row already exists, inspect and repair
the row before deciding whether to bind, defer, or start a new execution.

If Temporal reports an already-open execution for the intended issue, bind the
local row to that execution when it is safe to do so. Do not start a duplicate.

## Discovery Triggers

2607 should keep discovery explicit and bounded.

Allowed triggers:

- App startup repair pass;
- opening or refreshing a visible dashboard slice;
- targeted start after an operator action;
- targeted start after tracker refresh says an issue is executable;
- targeted repair for a selected issue.

Avoid:

- full-time polling daemon;
- full tracker scans on every App refresh;
- App-owned start loops independent from the Coordinator;
- Workflow starts for static lanes.

## Capacity Policy

The Coordinator consults configured capacity before starting new executions.

Initial caps come from `TASK-QUEUES.md`:

- `symphony-core`: up to 3 concurrent control-plane Activities, while
  serializing fact-changing writes per issue;
- `symphony-agent`: up to 3 concurrent agent runs, while allowing only one
  active agent attempt per issue;
- `symphony-local`: higher local concurrency, initially 8.

Coordinator capacity checks should focus on active `IssueWorkflow` starts and
known agent-run pressure. Do not over-model task queue internals before there
is measurement.

If capacity is unavailable:

- do not mutate tracker state;
- do not create a `starting` workflow row;
- return `capacity_deferred` with a visible reason;
- let the next explicit refresh/start attempt retry.

## Repair Flow

Repair reconciles local projection with Temporal and tracker. It does not
repair business state by moving tracker lanes.

Repair inputs:

- local `workflow_index` row;
- Temporal visibility/open execution lookup;
- Temporal Query when an execution is active and queryable;
- targeted tracker state read when needed.

Repair matrix:

| Local State | Temporal State | Action |
| --- | --- | --- |
| `starting` without `run_id` | not started or unknown | mark `stale_start` after timeout; reread tracker before retry |
| missing row | active execution exists | rebuild projection from Temporal visibility/query; do not start another execution |
| active row | active execution exists | keep row, refresh progress/freshness |
| active row | closed execution exists | mark `completed` or `failed` when close status is known; otherwise `closed_unknown` |
| active row | execution not found | mark `stale_missing`; reread tracker before retry |
| stale row | tracker still executable | allow new `workflow_id` and start retry |
| stale row | tracker is static | leave closed/stale projection; do not start |

Repair should be conservative. If the Coordinator cannot prove that a new start
is safe, return `repair_required` or `conflict` rather than creating a second
active execution.

## Temporal Interaction

Coordinator uses Temporal client APIs for:

- starting `IssueWorkflow`;
- checking whether a workflow execution is open;
- reading visibility/search attributes when available;
- querying active execution summaries when needed.

`IssueWorkflow` itself should run on `symphony-core`.

Start attributes should include enough metadata for targeted lookup:

- repo host/owner/name;
- issue number;
- tracker backend;
- from state;
- target kind;
- source slug;
- workflow start reason.

Do not depend on Temporal visibility as the only local read path for dashboard
views. SQLite remains the dashboard read model.

## SQLite Interaction

Coordinator writes only `workflow_index` and related freshness/progress fields
through `LocalStateProjector` or a narrow Coordinator-owned store boundary.

SQLite roles:

- duplicate-start guard;
- active workflow index for App and repair;
- projection of `workflow_id`, `run_id`, status, current state, freshness, and
  progress timestamps.

SQLite must not authorize business progression. A local row can block duplicate
starts, but it cannot prove that tracker transition, merge, PR link, or terminal
write succeeded.

## Tracker Interaction

Coordinator may read tracker state at durable boundaries:

- targeted start;
- repair before retry;
- explicit visible refresh;
- operator-action activation.

Coordinator must not write tracker state.

Tracker writes belong to `TrackerTransitionActivity`.

Manual tracker edits are treated as exception paths. Do not make every
Coordinator refresh pay for repeated full tracker reads just to detect rare
manual edits.

## App And Operator Interaction

The App may ask the Coordinator to:

- run an App-start repair pass;
- start executable work for a selected issue;
- repair a selected issue;
- refresh visible dashboard rows through bounded local/tracker/Temporal reads.

Human input, approve, request rework, and human-fix flows should still route to
Codex/operator flow first. The routed flow submits a typed action through the
Operator Action Bridge. The bridge then uses Temporal Update or a Coordinator
start boundary as appropriate.

The App should not directly choose lane transitions or run agents.

## Error Handling

Coordinator errors should be typed and observable.

Recommended error categories:

- `tracker_read_failed`;
- `local_guard_conflict`;
- `temporal_start_failed`;
- `temporal_lookup_failed`;
- `capacity_unavailable`;
- `static_state`;
- `invalid_state_for_start`;
- `repair_required`;
- `conflict`;
- `unhandled_error`.

Use `unhandled_error` for unexpected implementation/runtime failures. Avoid
calling this category `bug` in user-facing or tracker-facing state.

## Acceptance Checks

- Static tracker states do not start a Workflow by default.
- Executable tracker states can start an `IssueWorkflow` through Coordinator.
- Coordinator stores `workflow_id` and returned `run_id`.
- Two concurrent start attempts for the same issue cannot create two active
  workflow rows.
- A Temporal already-open execution can be rebound into SQLite projection.
- `starting` rows without `run_id` become `stale_start` after timeout.
- Active rows whose Temporal execution is gone become `stale_missing` or a
  closed status.
- Capacity deferral does not create a false `starting` row.
- Coordinator never calls agent runners directly.
- Coordinator never writes tracker state directly.
- App start repair works on bounded visible/configured scope, not an
  unbounded full-time scanner.

## Done Means

- Coordinator start/repair DTOs exist;
- executable/static classification is centralized;
- `workflow_id` construction is centralized;
- SQLite active guard is used before Temporal start;
- Temporal `run_id` is captured after start;
- targeted repair covers known stale local states;
- App/backend has a narrow Coordinator entrypoint;
- old autopilot start/resume behavior is not preserved as a second scheduler.
