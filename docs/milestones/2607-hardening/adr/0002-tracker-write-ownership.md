# ADR 0002: Tracker Write Ownership

Status: Proposed

## Context

Tracker writes are currently spread across lane, review, merge, doctor, and
project command paths. This makes state transitions harder to audit and makes
performance worse because each path tends to perform its own reads and
readbacks.

## Decision

Symphony owns tracker writes through `TrackerTransitionActivity`.

Shea and extension nodes return structured proposals and evidence. They may
recommend graph edges or the next core node, but `IssueWorkflow` decides and
`TrackerTransitionActivity` applies tracker transitions and field updates.

The transition path separates proposal, decision, and commit.

## Consequences

- Tracker transition rules can be tested in one place.
- Extension nodes cannot bypass safety checks.
- Extension nodes can still influence workflow direction without direct write
  authority.
- Temporal workflow state and query-backed snapshots can be shared during App
  reads.

## Follow-Up

- Define `TrackerTransitionActivity`.
- Define transition evidence requirements.
- Define restricted commits that only Symphony lanes may perform.
- Decide which existing write paths migrate first.
