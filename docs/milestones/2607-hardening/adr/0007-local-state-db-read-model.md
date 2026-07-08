# ADR 0007: Local State DB Read Model

Status: Proposed

## Context

Temporal gives Symphony a durable workflow runtime with history, replay,
queries, signals, updates, retries, and activity execution. That removes the
need for a hand-rolled orchestration state engine.

However, the App also needs fast local aggregate reads: dashboard rows across
many issues, tracker cache, PR summaries, artifact indexes, freshness markers,
and future async export/sync surfaces. Those are not the same problem as a
single workflow's authoritative state.

## Decision

Use a local SQLite database as the 2607 read model/cache/index layer.

- Temporal remains the authoritative local workflow runtime.
- Temporal Query is preferred for one workflow's current runtime state.
- SQLite backs dashboard materialized views, tracker cache, PR summary cache,
  artifact indexes, workflow indexes, and freshness markers.
- Process memory may cache hot UI/runtime values but is not required for
  correctness.
- Filesystem artifacts remain the body store for large logs, transcripts,
  patches, traces, and reports.
- Redis is deferred until a measured multi-process hot-cache or pub/sub need
  exists.

Default DB path:

```text
~/.shea/state/symphony.db
```

## Consequences

- App refresh can avoid repeated GitHub Project reads, artifact directory
  scans, and heavyweight command paths.
- SQLite must stay rebuildable from Temporal history/query, tracker reads, and
  artifacts.
- Workflow decisions must not depend on SQLite as the source of truth.
- Tracker writes still go through `TrackerTransitionActivity`.
- Query handlers stay deterministic and do not perform filesystem, tracker, or
  SQLite I/O.

## Follow-Up

- Define a small `LocalStateStore` interface.
- Add the initial schema for workflow, tracker, artifact, and activity indexes.
- Update App snapshot code to prefer SQLite materialized dashboard reads.
- Keep issue detail state backed by Temporal Query with lazy artifact reads.
- Add freshness markers and explicit refresh paths.

