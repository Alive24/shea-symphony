# ADR 0002: Tracker Write Ownership

Status: Proposed

## Context

Tracker writes are currently spread across lane, review, merge, doctor, and
project command paths. This makes state transitions harder to audit and makes
performance worse because each path tends to perform its own reads and
readbacks.

## Decision

Symphony owns tracker writes.

Shea and extension nodes return structured proposals and evidence. They may
recommend graph edges or the next core node, but Symphony validates and applies
tracker transitions and field updates.

The transition path separates proposal, decision, and commit.

## Consequences

- Tracker transition rules can be tested in one place.
- Extension nodes cannot bypass safety checks.
- Extension nodes can still influence workflow direction without direct write
  authority.
- A single tracker snapshot can be shared during a runtime tick.

## Follow-Up

- Define `TrackerCommand` or equivalent.
- Define transition evidence requirements.
- Define restricted commits that only Symphony lanes may perform.
- Decide which existing write paths migrate first.
