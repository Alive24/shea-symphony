# T2607-02 Local State DB

Status: Draft

## Purpose

Create the local SQLite read model/cache/index used by the App, Workflow
Coordinator, Activity projection, artifact lookup, and local health tools.

SQLite is not a workflow engine, not tracker truth, and not a domain model. It
is a rebuildable local read model with a typed access boundary.

## Goal

- initialize `~/.shea/state/symphony.db`;
- provide built-in schema versioning and minimal migrations;
- expose typed reader/projector/admin boundaries;
- support dashboard snapshots without tracker/Temporal fanout;
- support active Workflow duplicate-start guards;
- keep the DB rebuildable from Temporal, tracker, and artifacts.

## Non-Goals

- No ORM.
- No external migration service.
- No tracker state authorization.
- No workflow progression decisions from SQLite alone.
- No storage of full transcripts, diffs, issue bodies, comments, Project field
  dumps, review reports, or large test logs.
- No independent async projector loop in 2607.
- No cloud sync implementation.

## Expected Code Areas

Exact paths should follow repo inspection, but ownership should remain:

- `symphony` runtime module/crate owns schema, migrations, DTOs, and typed DB
  access;
- App/Tauri backend calls `LocalStateReader`, not raw SQL;
- Activities/backend projection code calls `LocalStateProjector`;
- CLI/admin fallback calls `LocalStateAdmin`;
- UI components do not mutate SQLite directly.

## Location And Config

Default path:

```text
~/.shea/state/symphony.db
```

Config override should follow existing precedence:

1. workspace-local config;
2. repo team shared `.shea` config;
3. global `~/.shea` config.

The DB must not live inside the canonical repository worktree by default.

## Schema Versioning

Use a small internal migration table or `meta` keys:

```text
meta(
  key,
  value
)
```

Required metadata:

- `schema_version`;
- `created_at`;
- `updated_at`;
- optional `last_rebuild_at`;
- optional `last_health_check_at`.

Migrations must be:

- idempotent;
- ordered;
- local-only;
- owned by the Symphony runtime;
- safe to run during App startup and worker startup.

## Initial Tables

Start with:

```text
workflow_index(
  workflow_id,
  run_id,
  repo_id,
  issue_ref,
  from_state,
  target_kind,
  current_state,
  active_step,
  waiting_kind,
  source_ref,
  started_at,
  last_progress_at,
  status,
  terminal_outcome,
  freshness,
  updated_at
)

artifact_index(
  artifact_id,
  workflow_id,
  repo_id,
  issue_ref,
  kind,
  path,
  summary,
  created_by_step,
  created_at
)

tracker_cache(
  repo_id,
  issue_ref,
  tracker_backend,
  tracker_state,
  title,
  pr_number,
  pr_state,
  pr_relation_confirmed_at,
  updated_at,
  freshness
)

activity_progress(
  workflow_id,
  activity_id,
  activity_kind,
  target_ref,
  mutation_id,
  outcome,
  status,
  attempt_count,
  last_heartbeat_at,
  next_retry_at,
  summary
)

meta(
  key,
  value
)
```

Use text columns for IDs and enum values at the DB boundary, with typed DTOs in
Rust/Swift/TypeScript boundaries as appropriate.

## Indexes And Guards

Primary keys:

- `workflow_index.workflow_id`;
- `tracker_cache(repo_id, issue_ref)`;
- `artifact_index.artifact_id`;
- `activity_progress(workflow_id, activity_id)`;
- `meta.key`.

Indexes:

- `workflow_index(repo_id, issue_ref, status)`;
- `workflow_index(current_state, waiting_kind)`;
- `artifact_index(workflow_id)`;
- `artifact_index(repo_id, issue_ref)`;
- `activity_progress(workflow_id, mutation_id)`;
- `tracker_cache(freshness)`.

For active workflow statuses, enforce one active workflow row per
`(repo_id, issue_ref)` with a partial unique index when SQLite support and the
chosen library make that practical. If not, enforce the same invariant through
a typed transaction guard.

The active guard is a duplicate-start guard for Workflow Coordinator. It is not
workflow truth.

## Workflow Index Status

Initial enum:

- `starting`;
- `running`;
- `completed`;
- `failed`;
- `start_failed`;
- `stale_start`;
- `stale_missing`;
- `closed_unknown`.

Do not represent static tracker waiting lanes through `workflow_index.status`.
Use `tracker_cache.tracker_state` for `Backlog`, `Human Review`, and normal
`Need Human Input`.

## Typed Access Boundary

Do not expose raw SQL to UI components or workflow business logic.

Initial interfaces:

```text
LocalStateReader:
  get_dashboard_snapshot(filter) -> DashboardSnapshot
  get_issue_index(issue_ref) -> IssueLocalIndex
  list_human_todo(filter) -> Vec<DashboardIssueSummary>
  list_artifacts(issue_ref) -> Vec<ArtifactRefSummary>

LocalStateProjector:
  project_workflow_summary(summary)
  project_tracker_cache(entry)
  project_artifact_ref(ref)
  project_activity_progress(progress)
  mark_stale(scope, reason)

LocalStateAdmin:
  check_health()
  migrate()
  rebuild(scope)
  compact()
```

Implementation preference:

- lightweight SQLite library;
- current preference: `rusqlite`;
- handwritten SQL;
- typed DTO/repository functions;
- no ActiveRecord-style object persistence.

## Projection Policy

2607 starts with synchronous projection.

When a runtime boundary produces data that affects dashboard, tracker cache,
artifact index, PR summary, or activity progress, it should project that result
before reporting the read model as fresh.

Projection failures should:

- not change Workflow truth;
- not authorize tracker changes;
- mark affected rows `stale` or `failed`;
- surface concise diagnostics to App/detail views.

Do not introduce an independent async projector loop in 2607.

## Rebuild Policy

SQLite is durable but rebuildable.

Rebuild inputs:

- Temporal workflow visibility/query/history when available;
- current tracker reads;
- artifact directory metadata and filenames;
- local config.

Rebuild may be partial. Rebuilt rows should carry freshness metadata. A rebuild
does not need to recreate every old dashboard optimization.

## Health Checks

`LocalStateAdmin.check_health()` should verify:

- DB file is reachable;
- schema version is supported;
- required tables exist;
- required indexes/guards exist;
- a small read/write transaction can run where appropriate;
- stale projection count can be reported.

Health checks should not scan full artifact directories or all tracker issues.

## Acceptance Checks

Minimum checks:

- DB initializes at the configured path;
- migrations are idempotent;
- required tables and indexes exist;
- reader methods return empty snapshots on a fresh DB;
- projector methods can insert/update sample rows;
- active workflow duplicate guard works for active statuses;
- completed/failed workflow rows do not block new episode-scoped
  `workflow_id`s;
- admin health check returns typed status;
- corruption/unavailable DB maps to typed errors;
- SQLite never acts as tracker transition authority.

## Rollback And Compatibility

This package may add SQLite and typed access code without changing existing
runtime behavior.

If local DB initialization fails, App/runtime should surface a clear local-state
error and avoid pretending dashboard snapshots are fresh. It should not fall
back to hidden tracker-wide scans without explicit refresh behavior.

## Done Means

- schema and migration path exist;
- typed reader/projector/admin boundaries exist;
- active workflow guard exists;
- basic dashboard/read-model DTOs can be produced;
- local state can be health-checked and rebuilt;
- no workflow progression or tracker transition depends on SQLite as truth.
