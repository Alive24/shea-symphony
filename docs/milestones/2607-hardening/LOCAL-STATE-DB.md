# Local State DB

Status: Draft

## Purpose

Define the local SQLite layer for 2607 Hardening.

SQLite is a local read model, cache, and index. It is not the workflow
orchestrator, not the tracker source of truth, and not a replacement for
Temporal history.

## Layering

2607 uses four different storage/read layers:

```text
Temporal Workflow state and history
  -> authoritative per-issue runtime state, replay, retry, signals, updates

SQLite local state DB
  -> dashboard materialized view, tracker cache, artifact index, local indexes

Process memory
  -> hot cache and live progress inside the currently running App/worker

Filesystem artifacts
  -> large bodies: transcripts, logs, reports, patches, traces
```

The important rule is that these layers do not compete.

- Temporal decides workflow progression.
- `TrackerTransitionActivity` commits external tracker state.
- SQLite helps the App and local tooling read quickly.
- Memory accelerates the current process only.
- Filesystem artifacts hold data too large for Temporal history or SQLite rows.

## Why SQLite

The performance problem is not only single-issue workflow state. Temporal Query
already handles that well by reading deterministic Workflow state, replaying
history when necessary, and preventing hidden I/O inside query handlers.

The slow local paths are broader:

- dashboard refresh across many issues;
- tracker cache reads that should not hit GitHub Project on every render;
- artifact reference lookup without scanning directories;
- PR summary cache;
- local UI materialized views;
- future async export or cloud sync of local operational summaries.

SQLite is the right 2607 default because it is local, durable, inspectable,
zero-service, fast enough for dashboard reads, and easy to rebuild.

## Temporal Query vs SQLite

Use Temporal Query for one workflow's authoritative runtime state:

- current tracker state last confirmed by `TrackerTransitionActivity`;
- active step;
- waiting state;
- attempt summaries;
- recent artifact refs;
- terminal or retry status.

Use SQLite for local aggregate/read-model state:

- top-level dashboard lists;
- human todo aggregation across workflows;
- tracker cache;
- PR summary cache;
- artifact index;
- workflow index;
- freshness markers.

Temporal Visibility/Search Attributes may help find workflows by metadata, but
they are not a replacement for the local dashboard read model in 2607.

## Recommended Location

Default local path:

```text
~/.shea/state/symphony.db
```

The path may be overridden by workspace-local config. It should not live inside
the canonical repository worktree.

## Minimal Schema Direction

Start with five small tables that support known read paths:

```text
workflow_index(
  workflow_id,
  repo_id,
  issue_ref,
  current_state,
  active_step,
  waiting_kind,
  last_progress_at,
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

Add columns only when a read path needs them. Do not store full transcripts,
diffs, review reports, issue bodies, comments, or Project field dumps in
SQLite.

Primary keys:

- `workflow_index.workflow_id`;
- `tracker_cache(repo_id, issue_ref)`;
- `artifact_index.artifact_id`;
- `activity_progress(workflow_id, activity_id)`;
- `meta.key`.

Indexes:

- `artifact_index(workflow_id)`;
- `artifact_index(repo_id, issue_ref)`;
- `activity_progress(workflow_id, mutation_id)`;
- `tracker_cache(freshness)`;
- `workflow_index(current_state, waiting_kind)`.

Do not add a dedicated `tracker_mutation_log` table in the initial schema.
Temporal history is the durable mutation attempt ledger. SQLite projects only
the current or recent observable state needed by dashboard/detail surfaces,
using `activity_progress`, `tracker_cache`, and artifact refs.

If a later measured need appears for cross-workflow mutation history queries,
add a projection table then.

## Identity Shape

Do not use naked issue numbers at typed boundaries.

Recommended DTO identity:

```text
RepoId {
  host
  owner
  repo
}

IssueRef {
  tracker_backend
  repo_id
  number
}

WorkflowId = "issue:<repo-id>:<number>"
```

SQLite may denormalize these values into text columns. Code boundaries should
use typed DTOs.

## Freshness Enum

Use a small enum first:

- `fresh`;
- `stale`;
- `refreshing`;
- `failed`;
- `unknown`.

Do not add more freshness states until a read path needs them.

## Access Layer

2607 should use a typed DB access layer, not an ORM.

SQLite is a local read model/cache/index, not a business fact layer or domain
model. Do not introduce ActiveRecord-style object loading/saving or a heavy
ORM.

Recommended shape:

- `LocalStateReader` for App/Tauri backend reads;
- `LocalStateProjector` for Activity/backend result projection writes;
- `LocalStateAdmin` for health checks, schema migration, rebuild, and compact
  operations.

Recommended initial methods:

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

The implementation should use a lightweight SQLite library, with a current
preference for `rusqlite`, handwritten SQL, and typed DTO/repository functions.

Schema versioning and minimal SQL migrations are owned by the Symphony runtime.
Do not introduce an external migration service in 2607.

Keep structured filter fields as columns. Small JSON summaries are acceptable
for backend-specific metadata or UI display payloads, but large payloads and
fields that need filtering should not be hidden in JSON.

## Write Policy

SQLite updates should happen through explicit Symphony runtime boundaries:

- workflow/activity completion projections;
- tracker refresh/cache Activities;
- artifact creation/indexing helpers;
- App-triggered refresh commands that call Symphony/Temporal boundaries.

Do not let UI components mutate SQLite directly.

Do not let SQLite writes imply tracker state mutation. Tracker mutation still
goes through `TrackerTransitionActivity`.

SQLite may accelerate reads, but must not authorize workflow progression or
tracker transitions. Workflow decisions depend on Temporal state, Activity
results, and targeted tracker readback, not SQLite cache contents.

Activity and backend code may read SQLite for non-authoritative optimization,
but any state-changing decision must be validated through the proper Temporal
or tracker boundary.

SQLite projection may receive concurrent requests. Writes should use short,
retryable transactions and remain non-authoritative. Projection failure should
mark affected read-model rows stale or failed rather than changing Workflow
truth.

## Projection Model

2607 starts with synchronous projection.

When an Activity or backend command produces a result that changes dashboard,
tracker-cache, artifact-index, PR-summary, or activity-progress data, the same
runtime boundary should update SQLite before returning or before reporting the
projection as fresh.

Do not introduce an independent async projector loop in 2607. That would risk
recreating the hand-rolled orchestration machinery this milestone is removing.

Keep projection code factored behind a small interface so a later milestone can
add async replay, export, or cloud sync without changing workflow semantics.

## Freshness Model

Dashboard reads are eventually consistent.

Ordinary dashboard render reads the SQLite materialized view. It should not
fresh-scan tracker, Temporal, artifact directories, or worktrees on every
render.

SQLite rows that mirror external or projected state should carry freshness
metadata such as:

- `updated_at`;
- `source`;
- `source_updated_at`;
- `freshness`;
- optional `last_refresh_error`.

Selected issue detail and explicit refresh paths may ask Temporal/tracker for
authoritative state. Top-level dashboard should show stale/fresh/failed status
rather than hiding refresh uncertainty.

## Rebuild Policy

The SQLite DB is durable but rebuildable.

If the DB is lost or corrupted, Symphony should be able to reconstruct useful
state from:

- Temporal workflow histories and queries;
- current tracker reads;
- artifact directories and indexes embedded in filenames/metadata where
  available.

Rebuild may be partial and freshness-marked. It does not need to recover every
old dashboard optimization.

Do not full-rebuild SQLite on every App or worker start. Startup should perform
lightweight schema and health checks. Rebuild should happen when the DB is
missing, corrupt, schema-incompatible, or explicitly requested by the operator
or a recovery path.

## Memory Cache

Memory is allowed as a hot cache above SQLite for:

- currently displayed dashboard rows;
- live worker progress;
- recent query results;
- debounced refresh state.

Memory must not be required for correctness. If the App or worker restarts,
Temporal plus SQLite plus artifacts should be enough to continue.

## Deferred Redis

Redis is not a 2607 default.

It may become useful later for multi-process hot pub/sub, very high-frequency
live updates, or shared process-local cache across independent frontends. Today
it adds a service, lifecycle, persistence, and consistency surface without
solving a proven bottleneck better than SQLite plus memory.

## Cloud Sync

SQLite can support a later async sync/export process, but 2607 should not make
cloud sync authoritative.

If sync appears in 2607, it should read from SQLite/materialized summaries and
artifact refs. It should not bypass Temporal, mutate tracker state, or become a
second workflow history.

## Non-Goals

- No second workflow engine.
- No replacement for Temporal Query.
- No replacement for GitHub Project or future tracker adapters.
- No large artifact body store.
- No direct UI-owned mutation path.
- No Redis dependency in 2607.
