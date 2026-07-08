# ADR 0004: App And CLI Separation

Status: Proposed

## Context

The App is the primary operator surface. Temporal local runtime is the reliable
execution boundary. If the App directly mutates tracker state or worktrees, the
system gains another source of truth.

## Decision

The Tauri backend command layer may call Temporal client APIs directly. Do not
introduce an independent local Symphony service in 2607.

The App is a display, routing, and local operation surface. It may open
Codex/operator flows for human input, approval, human fixes, rework, and human
doctor work, but it does not implement those workflow semantics directly. The
routed flow calls Symphony/Temporal interfaces with structured results.

CLI is admin/dev fallback only. It does not own workflow product operations.
Existing product commands should either disappear, become compatibility shims,
or call the same Temporal start/query/signal/update boundary as the App.

## Consequences

- App refresh must not trigger hidden write operations.
- App surfaces should consume status snapshots and state-grouped workflow
  snapshots.
- Human review/rework policy stays outside App UI code.
- Write paths stay testable as Temporal Activities.
- CLI no longer preserves autopilot/main/review/merge loop semantics as a
  parallel product surface.

## Follow-Up

- Define Tauri backend command allowlist.
- Define Temporal Query-backed issue detail reads for 2607.
- Define SQLite-backed dashboard read model for 2607.
- Audit existing App read surfaces for heavy command paths.
