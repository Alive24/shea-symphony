# ADR 0004: App And CLI Separation

Status: Proposed

## Context

The App is the primary operator surface. Temporal local runtime is the reliable
execution boundary. If the App directly mutates tracker state or worktrees, the
system gains another source of truth.

## Decision

The Tauri backend command layer may call Temporal client APIs directly. Do not
introduce an independent local Symphony service in 2607.

CLI is admin/dev fallback only. It does not own workflow product operations.

## Consequences

- App refresh must not trigger hidden write operations.
- App surfaces should consume status snapshots and state-grouped workflow
  snapshots.
- Write paths stay testable as Temporal Activities.

## Follow-Up

- Define Tauri backend command allowlist.
- Define Temporal query-backed workflow snapshot for 2607.
- Audit existing App read surfaces for heavy command paths.
