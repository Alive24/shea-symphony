# 2607 Code Ownership Map

This map keeps 2607 hardening work from sliding back into the MVP CLI and
autopilot structure while preserving the old code as reference material until
the Temporal migration is complete.

## Forward Development Home

`src/main.rs` is the 2607 Temporal worker runtime entrypoint. The default binary
should start the Symphony Temporal worker runtime, not dispatch the old CLI
command graph.

`src/symphony/**` is the forward development home for the 2607 Temporal spine:

- Temporal client and runtime setup;
- workflow and activity definitions;
- task queue registration;
- small workflow/activity DTOs;
- Coordinator-facing runtime boundaries;
- future operator action bridge, query, signal, and update surfaces.

New runtime behavior should land here first unless there is a stronger existing
owner.

## Shared Substrate And Contracts To Re-Express

Some MVP-era modules contain proven behavior that must be captured as contracts,
tests, and operational acceptance evidence. They may also contain bounded Rust
components worth reusing:

- `src/config.rs`
- `src/workflow.rs`
- `src/tracker/**`
- `src/workspace.rs`
- `src/artifacts.rs`
- focused helper modules with stable data or GitHub semantics

Reuse is a reviewed implementation choice, not a default migration strategy.
Prefer small protocol-independent types, parsers, adapters, and helpers that can
be extracted behind the new typed boundary with focused tests. Do not reuse an
MVP module when that would preserve old lane/runtime ownership, broad command
APIs, hidden state authority, or the product CLI command graph. External effects
still belong in Activities or operator-facing client surfaces, not deterministic
Workflow code.

## Legacy To Mine Or Delete

The old CLI/autopilot execution structure is reference material, not the target
architecture for new 2607 behavior:

- `src/commands/**`
- `src/lanes/**`
- old `src/orchestrator.rs` dispatch-loop behavior;
- MVP-era `autopilot` loop and lane runner code;
- old binary dispatcher patterns formerly rooted in `src/main.rs`.

These modules may be read to preserve proven tracker, handoff, review, merge,
and recovery semantics. Do not add new 2607 runtime features there unless the
issue explicitly says it is maintaining the protected MVP line.

## Context Preservation

The old entrypoint and CLI behavior are not lost when the 2607 default binary
changes:

- the protected `2606-MVP` branch remains the dogfood runtime reference;
- git history preserves the previous `src/main.rs` dispatcher and related code;
- old modules may stay temporarily as inactive migration reference until
  explicitly removed, but are not runtime dependencies;
- durable issue and PR review trails record intentional entrypoint decisions.

Physical relocation of legacy code should be a separate mechanical issue after
the Temporal skeleton and local state contracts stabilize. Until then, prefer
logical ownership boundaries over large path churn.

## Agent Guidance

When implementing 2607 hardening issues:

- put new Temporal runtime code under `src/symphony/**`;
- keep `src/main.rs` thin and runtime-entrypoint-shaped;
- re-express required MVP behavior through new typed boundaries and tests,
  selectively reusing bounded Rust components when the ownership review passes;
- do not extend MVP CLI/autopilot lanes as the new architecture;
- record deliberate exceptions in the issue or PR evidence.
