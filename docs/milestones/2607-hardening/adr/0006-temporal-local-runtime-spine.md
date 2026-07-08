# ADR 0006: Temporal Local Runtime Spine

Status: Proposed

## Context

Shea Symphony's MVP proved that the workflow can run, but the hand-rolled
orchestration loop creates runtime responsibilities that Temporal already
solves: durable state, retry, waiting, signals, queries, activity history, and
cancellation.

The protected 2606 MVP branch is the fallback. 2607 should not preserve the old
loop as a second durable runtime.

## Decision

Use local Temporal as the 2607 Symphony runtime spine.

- `IssueWorkflow` covers every standard Shea Symphony state from the start.
- Side effects run through Temporal Activities.
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
- Temporal history becomes the primary local execution trace.
- Large artifacts stay in the local artifact store and are referenced from
  Temporal history.
- The App becomes the main product operation surface.
- CLI no longer owns workflow product semantics.

## Follow-Up

- Define `IssueWorkflow` state handling.
- Define the Activity list and payload contracts.
- Define Tauri backend command allowlist.
- Define local Temporal startup and worker startup behavior.
- Remove or reduce old loop code as Temporal paths land.
