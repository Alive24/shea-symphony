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

## Implemented Migration V1

Migration v1 is the first executable schema. `rusqlite` owns connection and
transaction lifetimes, SeaQuery's SQLite builder is the single executable
schema definition, and `PRAGMA user_version` is the only version authority.
There is no parallel descriptor DSL or `meta.schema_version` row.

The database is machine-shared. Every repo-owned table includes
`workspace_runtime_id`, and typed `WorkspaceRuntimeId` values remain stable
across App restarts. `IssueRef` storage keys include tracker backend,
repository identity, and issue number; short `#123` forms are display-only.

The five v1 tables are:

- `workflow_index`: required execution, runtime/repo/issue, lane, activation
  provenance (`source_ref`, `source_tracker_revision`), status, freshness, and
  timestamp fields; nullable run/wait/progress/outcome/operator-action fields;
- `artifact_index`: required artifact, runtime/repo/issue, kind/path, and
  creation fields; nullable workflow, summary, and creating-step fields;
- `tracker_cache`: required runtime/repo/issue, backend/state/title,
  timestamp, and freshness fields; nullable PR fields;
- `activity_progress`: required runtime/workflow/activity, kind/target/status,
  and attempt fields; nullable mutation/outcome/heartbeat/retry/summary fields;
- `meta(key, value)`, initialized with RFC 3339 UTC `created_at` and
  `updated_at` rows.

Primary keys are `workflow_index(workflow_id)`,
`artifact_index(artifact_id)`,
`tracker_cache(workspace_runtime_id, repo_id, issue_ref)`,
`activity_progress(workflow_id, activity_id)`, and `meta(key)`.

Every lookup index begins with `workspace_runtime_id`. The deliberate exception
is the partial unique active guard on `(repo_id, issue_ref)` for statuses
`starting` and `running`: it is machine-wide across runtime scopes.

Use `workflow_index` as a local lifecycle projection index. It records the
human-readable Temporal Workflow ID plus the Temporal-native `run_id` and
authoritative execution start time only after current-Describe evidence is
available. It supports fast local diagnostics, active-execution hints, and
reconciliation input; the Coordinator still uses Temporal start/idempotency and
current execution evidence to decide whether an execution exists.

`workflow_id` is the primary Symphony execution identity. `run_id` is stored
for exact Temporal execution lookup, not as the product-level identity.

For active statuses, v1 enforces one active workflow row per
`(repo_id, issue_ref)` with a partial unique index.
This is a machine-wide local conflict signal and App read model, not the
authoritative workflow fact. It cannot reserve a Temporal start, reject a
start, or prove that no active execution exists.

Initial `workflow_index.status` enum:

- `starting`;
- `running`;
- `completed`;
- `failed`;
- `start_failed`;
- `stale_start`;
- `stale_missing`;
- `closed_unknown`.

Do not use `workflow_index.status` to represent static tracker waiting lanes.
Static waits such as `Backlog`, `Human Review`, and normal `Need Human Input`
belong in `tracker_cache.tracker_state`.

The v1 lifecycle projector writes only `running`, `completed`, `failed`, and
`closed_unknown` from Describe-backed observations. `starting` remains a schema
spelling for v1 compatibility, not a SQLite-owned pre-start reservation. No
v1 projection writer may create it from a start attempt or response.

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

WorkflowId = "issue:<repo-slug>:<number>:pulse:<from-state>-to-<target-kind>:<YYYYMMDD-HHMMSSZ>:<source-slug>"
```

SQLite may denormalize these values into text columns. Code boundaries should
use typed DTOs. `workflow_id` is episode-scoped; issue identity should be
carried separately through `repo_id` and `issue_ref`.

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
- `LocalStateAdmin` for later health, explicit rebuild/recovery, and compact
  operations. Startup migration remains owned by `LocalStateDatabase`.

Recommended initial methods:

```text
LocalStateReader:
  get_dashboard_snapshot(filter) -> DashboardSnapshot
  get_issue_index(issue_ref) -> IssueLocalIndex
  list_human_todo(filter) -> Vec<DashboardIssueSummary>
  list_artifacts(issue_ref) -> Vec<ArtifactRefSummary>

LocalStateProjector:
  project_workflow_lifecycle(describe_backed_observation)
  project_tracker_cache(entry)
  project_artifact_ref(ref)
  project_activity_progress(progress)
  mark_stale(scope, reason)

LocalStateAdmin:
  check_health()
  rebuild(scope)
  compact()
```

The lifecycle boundary uses bundled `rusqlite`, SeaQuery SQLite schema
statements, and SeaQuery's rusqlite binder. Narrow private SQL is limited to
SQLite PRAGMA control and schema introspection that SeaQuery does not express.
No ORM or external migration service is used.

Each connection receives a five-second busy timeout, foreign keys enabled,
`synchronous = NORMAL`, and confirmed WAL mode. Missing migrations run in
short `IMMEDIATE` transactions and re-read `user_version` after locking.
Future versions, unversioned schema drift, corruption, bounded contention,
migration failure, and readback failure remain typed and non-destructive.

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

SQLite projection may receive concurrent requests. Writes use short bounded
transactions and remain non-authoritative; callers reconcile a conflict or
projection failure through Temporal/tracker boundaries rather than changing
Workflow truth or adding a SQLite retry scheduler.

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

### Describe-Backed `workflow_index` V1

`LocalStateProjector` is a synchronous, crate-private writer. It receives a
typed observation from a caller that has already read Temporal; it performs no
Temporal or tracker I/O itself.

- A current Describe-backed open execution creates or updates a `running` row
  with the described Run ID, authoritative `started_at`, `fresh` freshness,
  `workflow_execution` active step, and the observation time as `updated_at`.
- A current Describe-backed closed execution creates or transitions to
  `completed`, `failed`, or `closed_unknown`. V1 keeps close time only as input
  evidence; it does not add `closed_at`, failure-detail, or diagnostic columns.
- A Temporal StartResponse can confirm only an already-projected identical Run
  ID. A missing/different row returns `DescribeRequired` and makes no write.
- A definitive start failure returns a bounded typed no-write result. It does
  not create a start reservation, invent a Run ID or start timestamp, or store
  its diagnostic in `workflow_index`.
- Immutable activation facts are compared field-by-field. Stale Run evidence
  and projection conflicts leave the existing row unchanged. `completed` and
  `failed` do not regress; only `closed_unknown` may refine when a supported
  close classification later becomes available.

The partial active index remains a useful local duplicate diagnostic after a
projection is observed. It is never a precondition or authority for Temporal
start, progression, retry, cancellation, tracker transition, PR linkage, or
terminal business state.

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
