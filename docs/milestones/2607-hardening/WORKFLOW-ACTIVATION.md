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

The Coordinator constructs `workflow_id` with this exact grammar:

```text
issue:<encoded-host>/<encoded-owner>/<encoded-repo>:<issue-number>:pulse:<from-state>-to-<target-kind>:<YYYYMMDDTHHMMSSZ>:<source-kind>-<encoded-source-ref>
```

Examples:

```text
issue:github.com/Alive24/shea-symphony:123:pulse:todo-to-work:20260708T134218Z:tracker-project-rev-456
issue:github.com/Alive24/shea-symphony:123:pulse:merging-to-merge:20260708T150012Z:operator-action-action-789
```

Use `target-kind` rather than guaranteed final state. A `Todo` pulse may end
in `In Progress`, `Need to Clarify`, `Agent Review`, or `Need Human Input`.
The Workflow ID should describe why the pulse started, not pretend to know its
final state. Coordinator exclusively derives `work`, `review`, `rework`, or
`merge` from the observed executable tracker state; callers cannot supply a
target kind.

Repository components and `source-ref` use reversible URL-safe percent encoding
of their UTF-8 bytes. ASCII letters, digits, `-`, `.`, `_`, and `~` remain
readable; separators and other bytes are encoded without case folding,
slugification, replacement, hashing, or truncation. Enum spellings are stable
lowercase kebab-case.

Episode time is explicit UTC input at second precision. Identity construction
must not read a clock or regenerate the timestamp during an uncertain retry.
The same issue, observed state, episode time, source kind, and source reference
therefore produce the same ID. A new episode time or source identity produces a
new ID.

Shea limits the complete encoded Workflow ID to 256 UTF-8 bytes. Overflow is a
typed validation error even when Temporal is configured with a larger limit.
The audit reason is trimmed, non-empty provenance of at most 512 UTF-8 bytes;
it is never embedded in the Workflow ID.

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
- `Need to Clarify`;
- `Need Human Input`;
- `Human Review`;
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
- `Merging`: land/merge flow.

`Backlog` promotion to `Todo` creates an executable condition. `Todo` is the
workflow activation point. Doctor or reconciliation may perform a bounded
operation that later moves the tracker to an executable state, but Coordinator
does not treat `Need Human Input` itself as executable.

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

## Pure Coordinator Activation Contract

The first independently reviewable Coordinator slice accepts a validated
activation request plus an already-observed tracker state and revision. It does
not fetch tracker state or perform SQLite, Temporal, capacity, filesystem,
network, process, or App I/O.

An optional expected tracker state or revision is an optimistic precondition.
The pure result is one of:

- `Static`, with the observed static state and revision;
- `Executable`, with the observed state/revision, Coordinator-derived target
  kind, explicit episode time, source kind/reference, bounded audit reason, and
  validated `WorkflowId`;
- `StaleExpectation`, when either supplied expectation differs from the
  observation.

Only `Executable` contains executable activation facts or a Workflow ID.

## Workflow Coordinator

The Workflow Coordinator is a thin launcher and registrar. It should:

- read tracker states;
- identify executable states;
- construct the human-readable `workflow_id`;
- start `IssueWorkflow` executions only for executable states;
- use Temporal start/idempotency and a current execution Describe to establish
  execution identity and status;
- project verified Describe observations into `workflow_index` for local reads
  and reconciliation hints;
- use SQLite conflicts as diagnostics to reconcile against Temporal, never as a
  start reservation or execution authority;
- enforce at most one active execution per issue through Temporal-aware
  coordination, not an SQLite row alone;
- respect task queue and Activity concurrency limits;
- ignore static issues except for dashboard/read-model projection.

It must not run agents, choose business transitions, or directly write tracker
state. The Workflow owns orchestration decisions. Activities own side effects.

Task queue concurrency controls active work, not the number of static issues in
the tracker.

## Start Contract

Coordinator start is optimistic with Temporal as the authority:

```text
read tracker executable state
  -> derive workflow_id
  -> start Temporal Workflow
  -> Describe the current Temporal execution
  -> project the Describe-backed Open observation into workflow_index
```

The start response can prove that Temporal accepted a request and may carry a
Run ID, but it cannot supply the authoritative execution `started_at` required
by v1. Therefore a StartResponse never creates a `starting` row, replaces a
stored Run ID, or fabricates a start time. A matching already-projected Run can
be confirmed; otherwise the projection boundary returns `DescribeRequired`.

The Describe-backed SQLite row makes execution observation fast and locally
inspectable. Temporal start/idempotency, current Describe, and history remain
the execution facts; SQLite is not the authoritative workflow runtime.

Start conflicts:

- SQLite active-row conflict: treat it as a local diagnostic, read current
  Temporal evidence, and project/reconcile only that evidence; do not use it to
  reserve, reject, or retry a start;
- Temporal open Workflow already exists: bind to the existing execution and
  project its current Describe observation;
- Temporal closed Workflow with the same `workflow_id`: create a new activation
  episode with a new explicit timestamp or source identity; never invent a
  retry-time suffix.

## Repair Contract

The Coordinator/reconciliation boundary owns observation and may ask the
projector to materialize it. It does not repair business state by directly
moving tracker lanes or by issuing ad hoc SQLite lifecycle SQL.

Repair matrix:

| Local State | Temporal State | Action |
| --- | --- | --- |
| missing row | current Describe is open | project `running` from the described Run ID and start time; do not infer a prior SQLite reservation |
| missing row | current Describe is closed | project `completed`, `failed`, or `closed_unknown` from the described execution |
| running row | current Describe is the same or a newer Run | project the Open observation; only current Describe evidence can replace the stored Run ID |
| running row | current Describe is closed for the stored Run | project its bounded terminal classification |
| any row | StartResponse only or definitive start failure | return the typed no-write projection outcome; do not create a row |
| any row | conflicting, stale, or unavailable evidence | leave the row unchanged and reconcile through Temporal/tracker policy outside the projector |

Use conservative stale thresholds:

- `running` without heartbeat/progress beyond its configured TTL;
- Temporal visibility/query cannot confirm the recorded active execution.

The v1 lifecycle projector does not create `starting`, `stale_start`, or
`stale_missing` rows. Any later stale-row policy needs an explicit projection
contract rather than reintroducing a pre-start SQLite reservation.

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

Normal in-flight decisions rely on Temporal state, Activity results, and
artifact references. SQLite active-index rows may provide local diagnostic
hints, but cannot authorize an in-flight decision. Manual tracker edits are an
exception path; they should not make the normal path pay for repeated full
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
can start from `Todo`, `In Progress`, `Agent Review`, `Rework`, or `Merging`.
If one handler produces another executable state, continuing in the same
execution is allowed and can reduce handoff overhead.

Examples:

- `Todo` contract check passes and starts implementation, continuing into the
  `In Progress` handler;
- `In Progress` completes PR/link/handoff work and commits `Agent Review`;
- `Agent Review` passes and commits `Human Review`, then completes because
  `Human Review` is static with `completed_static_handoff`;
- `Merging` lands work and commits `Done`, then completes;
- an operational blocker commits `Need Human Input`, then completes.

## Non-Goals

- No live Workflow execution for every tracker issue.
- No long-lived idle Workflow for `Backlog`.
- No long-lived idle Workflow for `Human Review`.
- No App-side scheduler.
- No custom autopilot queue beside tracker plus Temporal.
