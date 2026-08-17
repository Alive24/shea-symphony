# T2607-02 Local State DB

Design Status: Draft

Implementation progress is tracked in `../STATUS.md`.

## Purpose

Provide Symphony's machine-local SQLite read model/cache/index. SQLite is
rebuildable and cannot authorize Workflow progression, tracker transitions,
PR linkage, merging, or terminal writes.

## Implemented Migration Slice

- `LocalStateDatabase` is a cloneable path and connection-policy handle;
  physical `rusqlite::Connection` values remain private.
- The default path is `~/.shea/state/symphony.db`. Relative overrides resolve
  below `~/.shea/`, while callers may inject an already resolved absolute path.
- Missing parent directories are created. Initialization never falls back to
  memory or a repository-local database.
- Every connection uses a five-second busy timeout, `foreign_keys = ON`,
  `synchronous = NORMAL`, and confirmed WAL journal mode.
- Bundled `rusqlite` owns connections and transactions. SeaQuery's SQLite
  builder is the only executable schema authority; SeaQuery's rusqlite binder
  handles parameterized metadata writes.
- Private fixed SQL is used only for SQLite PRAGMA/schema introspection where
  SeaQuery has no representation.
- `PRAGMA user_version` is the only schema-version authority.
- Missing migrations run in ordered, short `IMMEDIATE` transactions and
  re-read the version after acquiring the write lock.
- Migration v1 creates `workflow_index`, `artifact_index`, `tracker_cache`,
  `activity_progress`, and `meta` atomically.
- `meta` begins with RFC 3339 UTC `created_at` and `updated_at`; it never stores
  a duplicate schema version.

## Implemented Admin Slice

- `LocalStateAdmin` is a crate-internal library seam above
  `LocalStateDatabase`, not a product CLI command, Tauri command, or Temporal
  Activity.
- `check_health` opens only an existing database in SQLite read-only mode. It
  never creates a path, configures a connection, changes journal mode, runs a
  migration, repairs state, or replaces a file.
- Health returns typed current, migration-required, incompatible,
  unversioned-conflict, incomplete-schema, corruption, and unavailable-path
  outcomes. Current v1 readiness verifies the required table set and migration
  metadata without becoming a database-wide schema audit.
- `migrate` is explicit and delegates unchanged to
  `LocalStateDatabase::initialize`; it is the only Admin operation that may
  create a database or parent directory and apply supported forward migrations.
- T2607-07 owns any future Tauri command or operator-facing surface over this
  library. It must keep health read-only unless the caller explicitly requests
  migration or a later recovery operation.

## Scope And Identity

One local Temporal service/namespace and one SQLite database are shared per
machine. Repo-owned rows are explicitly scoped by typed, stable
`WorkspaceRuntimeId` and `RepoId` values. A `WorkspaceRuntimeId` survives App
restarts and is not a PID, timestamp, transient worktree path, or Worker
Identity.

`IssueRef::database_key()` includes tracker backend, repository identity, and
issue number. Short references such as `#479` are display-only.

`workflow_index` retains immutable activation provenance through required
`source_ref` and `source_tracker_revision`, plus nullable
`operator_action_ref`. These fields are identity evidence, not start authority.

## V1 Keys And Indexes

Primary keys:

- `workflow_index(workflow_id)`;
- `artifact_index(artifact_id)`;
- `tracker_cache(workspace_runtime_id, repo_id, issue_ref)`;
- `activity_progress(workflow_id, activity_id)`;
- `meta(key)`.

Workspace-scoped indexes:

- `workflow_index(workspace_runtime_id, repo_id, issue_ref, status)`;
- `workflow_index(workspace_runtime_id, current_state, waiting_kind)`;
- `artifact_index(workspace_runtime_id, workflow_id)`;
- `artifact_index(workspace_runtime_id, repo_id, issue_ref)`;
- `tracker_cache(workspace_runtime_id, freshness)`;
- `activity_progress(workspace_runtime_id, workflow_id, mutation_id)`.

The deliberate cross-scope invariant is a partial unique index on
`workflow_index(repo_id, issue_ref)` where status is `starting` or `running`.
It prevents two locally active rows across runtime scopes, but cannot authorize
or reserve a Temporal Workflow start.

Required v1 values use `NOT NULL`; fields not yet produced remain nullable.
Timestamps are RFC 3339 UTC text. V1 has no foreign keys or enum `CHECK`
constraints so out-of-order projection and compatible enum additions do not
require table reconstruction.

## Failure And Compatibility Semantics

- Empty version zero applies v1.
- Current version returns without a full schema/health scan.
- A future version returns `unsupported_schema_version` without mutation.
- Version zero with application objects returns
  `unversioned_schema_conflict`; migrations never hide drift with
  `IF NOT EXISTS`.
- Corrupt/malformed files return a typed corruption error and are not replaced.
- Lock contention ends at the configured timeout with `database_busy`.
- Schema and version updates roll back together on migration failure.
- Post-migration version confirmation has its own typed failure.

## Implemented Projection Slice

- `LocalStateProjector` is a concrete synchronous crate-private writer. It
  opens no Temporal/tracker client and exposes no `rusqlite::Connection` to its
  callers.
- `WorkflowLifecycleObservation` directly carries the immutable activation
  facts, caller-supplied `current_state`, and `observed_at`; no secondary
  execution-identity wrapper or fingerprint is introduced.
- Only current Describe-backed Open/Closed observations can materialize a v1
  row. Open creates/updates `running`; Closed creates/transitions to
  `completed`, `failed`, or `closed_unknown` with a bounded close
  classification.
- `started_at` is always the Describe-proven Temporal execution start time;
  `updated_at` changes only for a material projection. `waiting_kind` remains
  NULL and `last_progress_at` is unchanged in this summary slice.
- StartResponse is same-Run confirmation only. Missing/different rows return
  `DescribeRequired` without a write. Definitive start failure returns a
  bounded typed `StartFailureNotProjected` outcome and persists neither a
  diagnostic nor a fabricated Run ID/start timestamp.
- A short `BEGIN IMMEDIATE` transaction reads, compares immutable facts,
  validates the monotonic Run/status transition, writes when needed, reads the
  row back, and commits. The machine-wide active partial-index violation maps
  to a typed local conflict without weakening its cross-runtime issue scope.
- `completed` and `failed` do not regress. `closed_unknown` may refine when a
  supported close classification later arrives. Old-Run evidence and identity
  conflicts never overwrite a row.

## Implemented Active Query Slice

- `LocalStateReader` is a crate-internal, synchronous read boundary. It reuses
  `LocalStateAdmin` readiness diagnostics and opens only an existing, current
  database in SQLite read-only mode.
- A fully qualified repository/issue lookup returns only the shared
  `starting` or `running` status rows. A scoped list filters by
  `workspace_runtime_id` and `repo_id`, orders by stored `issue_ref` then
  `workflow_id` ascending, and returns at most 100 rows.
- Successful absence is possible only after readiness is established. Missing,
  uninitialized, corrupt, incomplete, unversioned-conflict, or incompatible
  state remains a typed unavailable/not-ready result.
- Reads do not infer lifecycle, freshness, latest Run, tracker state, or
  Temporal state, and they do not initialize, migrate, repair, project, write,
  or make network calls.

## Deferred Read-Model Ledger

Only active `workflow_index` reads are implemented in this slice. Absence from
an unimplemented surface is not evidence that the underlying work is fresh,
successful, or complete.

| Deferred surface | Known downstream owner |
| --- | --- |
| `tracker_cache` reads/projection completion | T2607-04 Tracker Transition Activity |
| `activity_progress` reads/projection completion | T2607-05 Agent Activity Boundary |
| `artifact_index` reads/projection completion | T2607-05 Agent Activity Boundary |
| Full `DashboardSnapshot` assembly and App exposure | T2607-07 App Integration |

## Deferred Slices

- `query`: tracker cache, activity progress, artifact index, and complete
  dashboard snapshots beyond the implemented active workflow index;
- `admin`: explicit rebuild/replace, compact, recovery, and manual checkpoint
  operations beyond the implemented health/explicit-migration boundary;
- `test/hardening`: measured contention, internal pooling decisions,
  corruption/rebuild, and performance work after real callers exist;
- `integration`: layered config resolves the final path and passes it to this
  boundary;
- cross-package: physical repo-scoped Temporal task queues and the one-App /
  Coordinator lease.

No Temporal Activity, Workflow routing, Tauri command, projection loop, pool,
tracker-wide fallback scan, or external migration service belongs to this
slice. Synchronous SQLite work must be kept off Tokio/Temporal async worker
threads when later integrations call it.

## Verification

Temporary-file tests cover initialization/no-op reinitialization, concurrent
initializers, exact columns/nullability/keys/indexes, connection PRAGMAs,
machine-wide active uniqueness, workspace-scoped tracker cache keys, future
versions, unversioned drift, malformed files, bounded contention, atomic
rollback, read-only health diagnostics, and explicit admin migration re-entry.
Tests inject all paths and do not touch the operator database or the canonical
worktree.
