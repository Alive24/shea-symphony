## Issue Setup

- UAT Required: Yes
- Assignee: Alive24
- Dependencies: None
- Related Parent Issue or Context: Promoted from the Issue Forge fixture Backlog seed.

## Issue Goal

Harden the Issue Forge promotion workflow in Shea Symphony CLI fixtures.

## Why Now

Operators need deterministic coverage before relying on promotion notes during live dogfood.

## Issue Context

The Backlog seed is non-dispatchable until an operator confirms promotion and supplies a full executable issue contract.

## Non-Negotiable Guardrails

- Do not treat Backlog seeds as executable work.
- Keep the Backlog to Todo status mutation as the final promotion write.

## Scope

### In Scope

- Fixture-backed `forge promote --dry-run` coverage.
- Checkbox-style review evidence sections.

### Out Of Scope

- Live tracker mutation from this fixture.

## Canonical References

### Target Repository / Package

- Alive24/shea-symphony

### Relevant Knowledge Sources

- docs/README.md
- .shea/contracts/workflow-capability.v1.md

### Relevant Code Paths

- src/main.rs
- src/quality_gate.rs
- tests/fixtures/workflows/promote.md
- tests/fixtures/tracker/promote.json
- tests/fixtures/issues/promoted.md

## Current State

The fixture Backlog issue exists only for dry-run promotion coverage.

## Deliverable Shape

A dry-run promotion report with a structured Promotion Note preview.

## Risks or Constraints

- Fixture promotion must not imply live Project status changed.

## Expected Outcome

- [ ] `forge promote --dry-run` accepts the promoted body and structured note inputs.
- [ ] The Promotion Note preview includes decisions, scope changes, dependency context, and readback summaries.

## Verification

### Completion Criteria

- [ ] The promoted fixture body passes the Todo Issue Quality Gate.
- [ ] Backlog seeds remain non-dispatchable until promoted.

### Functional Verification

- [ ] `cargo run -- forge promote '#241' --workflow tests/fixtures/workflows/promote.md --title "Harden Issue Forge promotion fixture" --body-file tests/fixtures/issues/promoted.md --operator-confirmation "promote it" --decision "Keep the promotion in place." --scope-change "Backlog seed becomes an executable Todo issue." --dependency-context "Dependencies: none." --readback-summary "Dry-run preview verified before write." --dry-run`
- [ ] `cargo test`

### UAT

- [ ] Confirm an operator can read the dry-run Promotion Note preview without live mutation.

### Context Verification

- [ ] Confirm Issue Forge shaping remains skill-owned and CLI promotion remains deterministic.
