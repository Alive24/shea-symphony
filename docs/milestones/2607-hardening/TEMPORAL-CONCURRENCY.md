# Temporal Concurrency

Status: Draft

## Purpose

Define the 2607 concurrency model.

Temporal is the durable concurrency spine. Symphony should not rebuild an
autopilot scheduler, tick loop, or local retry/resume framework beside
Temporal.

## Core Model

Use at most one active `IssueWorkflow` execution per issue at a time.

A single Workflow execution is a deterministic ordered state machine. It does
not mutate its own Workflow state concurrently. Signals and Updates into one
Workflow are appended to Workflow history and processed in history order.

Tracker remains the durable queue between Workflow activations. `IssueWorkflow`
is an executable orchestration episode, not a live long-running execution for
every issue from `Backlog` to `Done`.

Parallelism happens through:

- multiple `IssueWorkflow` executions;
- Activities;
- Child Workflows when a later design needs them;
- Worker pools.

Multiple executable issues may run concurrently as separate `IssueWorkflow`
episodes. Within one issue, read-only or non-conflicting Activities may run
concurrently when the active episode can safely join their results.

Workflow population is based on executable tracker states, not all
Shea-managed issues.

## Per-Issue Ordering

An active `IssueWorkflow` episode owns ordered per-issue decisions while it is
running:

- current state;
- next allowed action;
- retry/wait/reject decisions;
- lane handoff completion;
- when to request tracker transition or mutation;
- when to enter `Need Human Input`.

Workflow code must validate tracker state and allowed actions whenever it
handles a Signal or Update. History ordering prevents concurrent Workflow state
mutation inside one execution, but it does not remove the need for validation
against tracker state and the action context.

## Parallel Activities

Activities can run concurrently when they do not change the same external fact.

Good candidates:

- read-only tracker/PR checks;
- artifact indexing;
- log/artifact summarization;
- non-mutating doctor checks;
- independent cache refreshes;
- selected review/read-only analysis.

Avoid per-model-turn Activity modeling. Codex/Main/Review/Merge should remain
coarse Activity boundaries. Codex app-server and review backends own their
internal execution loops; Temporal tracks attempt boundaries, heartbeats,
outcome summaries, and artifact refs.

## Serialized External Fact Changes

External fact-changing operations should remain serial, idempotent, and
readback-verified for one issue:

- tracker transitions;
- PR-to-issue linking;
- merge/land;
- terminal writes;
- claim cleanup;
- tracker-visible evidence writes that must not duplicate.

These operations should use stable idempotency keys and targeted readback.
Activity success means the desired external fact is observed, not merely that a
command exited successfully.

## Signals And Updates

Signals and Updates into one active `IssueWorkflow` execution are ordered by
Workflow history.

Workflow handling still must validate:

- current state;
- allowed action;
- `OperatorActionContext` expiry and capability;
- payload schema;
- evidence refs;
- idempotency or duplicate submission policy.

Use Updates for state-changing operator actions that need synchronous
accepted/rejected feedback. Use Signals for low-risk fire-and-continue notes or
supplemental evidence.

## SQLite Concurrency

SQLite projection can be concurrent at request level, but it is not
authoritative.

DB writes should use short retryable transactions. Projection failures should
mark read-model freshness as stale or failed rather than changing Workflow
truth.

SQLite must not authorize workflow progression, tracker transition, PR link,
merge, or terminal writes.

## Worker Pools

Worker pools own parallel external work. Active `IssueWorkflow` episodes own
per-issue ordering. Tracker owns queueing between activations.

2607 should start with three task queues:

- `symphony-core` for `IssueWorkflow`, tracker transitions, PR-link mutation,
  and latency-sensitive workflow control Activities;
- `symphony-agent` for Codex Main/Rework/Merge runs and heavy Agent Review
  backend work;
- `symphony-local` for SQLite projection, artifact indexing, tracker cache
  refresh, and local health/admin/rebuild work.

Use Activity-level concurrency limits within each queue. Do not split into many
more queues until measurement proves contention or isolation needs.

Initial configurable defaults:

- `symphony-core`: up to 3 concurrent control-plane Activities, while
  serializing fact-changing writes per issue;
- `symphony-agent`: up to 3 concurrent agent runs, while allowing only one
  active agent attempt per issue;
- `symphony-local`: higher local concurrency, initially 8, with short retryable
  SQLite write transactions.

The concurrency policy should protect external systems:

- limit GitHub/tracker mutation concurrency;
- keep merge/land serialized per issue;
- avoid running duplicate PR-link mutations for the same issue/PR pair;
- let read-only cache/artifact work run with higher concurrency.

## Non-Goals

- No custom autopilot scheduler.
- No second durable retry/resume loop.
- No per-model-turn Temporal Activity graph.
- No SQLite-backed workflow coordinator.
- No App-side workflow scheduler.
- No long-lived idle Workflow execution for static tracker lanes.
