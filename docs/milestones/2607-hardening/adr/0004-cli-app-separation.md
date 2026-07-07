# ADR 0004: CLI And App Separation

Status: Proposed

## Context

The App is useful as an operator surface, but the reliable execution boundary
should stay in Symphony. If the App directly mutates tracker state or worktrees,
the system gains another source of truth.

## Decision

The CLI/Symphony runtime is the execution authority. The App is a controlled
consumer and operator control surface.

The App may control tick/autopilot execution. The App does not directly modify
tracker state or worktrees.

## Consequences

- App refresh must not trigger hidden write operations.
- App surfaces should consume status snapshots and state-grouped workflow
  snapshots.
- Write paths stay testable in Symphony.

## Follow-Up

- Define App command allowlist.
- Define read-only workflow snapshot for 2607, with future graph snapshot
  support in 2608.
- Audit existing App read surfaces for heavy command paths.
