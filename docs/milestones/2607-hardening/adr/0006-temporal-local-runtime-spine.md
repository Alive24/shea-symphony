# ADR 0006: Temporal Local Runtime Spine

Status: Proposed

## Context

Shea Symphony's MVP proved that the workflow can run, but the hand-rolled
orchestration loop creates runtime responsibilities that Temporal already
solves: durable state, retry, waiting, signals, queries, activity history, and
cancellation.

The protected 2606 MVP branch supplies the active App/CLI bootstrap during
development, the recovery baseline, and the acceptance oracle. It remains
outside the 2607 architecture; 2607 should not call or wrap the old loop as a
compatibility or second durable runtime. Reviewed reuse of bounded Rust
components remains allowed when they are extracted from that orchestration
ownership and placed behind new typed boundaries.

## Decision

Use local Temporal as the 2607 Symphony runtime spine.

- `IssueWorkflow` understands every standard Shea Symphony state from the
  start.
- Use at most one active `IssueWorkflow` execution per issue at a time.
- Tracker remains the durable queue and external state between workflow
  activations.
- `IssueWorkflow` is an executable orchestration episode, not a live execution
  for every issue from `Backlog` to `Done`.
- `workflow_id` is a human-readable, episode-scoped Symphony execution
  identity and Temporal Workflow ID. Temporal's returned `run_id` is retained
  for exact execution lookup.
- A thin Workflow Coordinator starts executable `IssueWorkflow` executions and
  records `workflow_id`/`run_id`; it does not run agents or choose business
  transitions.
- Coordinator start uses Temporal start/idempotency and current Describe as
  execution facts. SQLite active-row conflicts are local diagnostics, never
  pre-start reservations or start authority; Coordinator repair reconciles the
  projection against current Temporal evidence and tracker policy.
- Executable lane handlers are independently startable and internally
  chainable. Chaining is Workflow continuation, not a terminal outcome.
- Coding/review/merge/doctor work uses coarse Agent Activities with typed
  request/result DTOs, hard capability profiles, worktree leases when writing,
  layered heartbeats, and artifact refs for large evidence.
- Child Workflows are allowed but not the default. Promote subflows only when
  they need independent durable orchestration.
- Multiple executable issues run concurrently as separate Workflow episodes.
- Side effects run through Temporal Activities.
- Read-only or non-conflicting Activities may run concurrently; external
  fact-changing operations remain serial, idempotent, and readback-verified.
- Start with three task queues: `symphony-core`, `symphony-agent`, and
  `symphony-local`.
- Start with configurable concurrency caps of 3 for `symphony-core` and 3
  concurrent agent runs for `symphony-agent`, while preserving per-issue
  serialization for fact-changing operations.
- Tracker writes go through `TrackerTransitionActivity`.
- App operations use Tauri backend commands that call Temporal start, query,
  signal, or update APIs.
- CLI is admin/dev fallback only.
- Core runtime code uses the `symphony` naming boundary rather than introducing
  a separate `temporal_runtime` package name by default.
- Temporal Cloud is out of scope.
- No independent local Symphony service is introduced in 2607.

## Consequences

- The old autopilot/tick/resume loop becomes legacy-to-delete.
- Symphony does not rebuild an autopilot scheduler beside Temporal.
- Temporal history becomes the primary local execution trace.
- Worker pools own parallel external work; active `IssueWorkflow` executions
  own ordered per-issue decisions.
- Workflow population is based on executable tracker states, not all static
  Shea-managed issues.
- Long-running Coding Agent work is isolated from latency-sensitive
  control-plane and local projection work.
- Large artifacts stay in the local artifact store and are referenced from
  Temporal history.
- The App becomes the main product operation surface.
- CLI no longer owns workflow product semantics.

## Follow-Up

- Define `IssueWorkflow` state handling.
- Define Activity concurrency limits and worker-pool policy for the three
  starting task queues.
- Define the Activity list and payload contracts.
- Define Tauri backend command allowlist.
- Define local Temporal startup and worker startup behavior.
- Remove or reduce old loop code as Temporal paths land.
