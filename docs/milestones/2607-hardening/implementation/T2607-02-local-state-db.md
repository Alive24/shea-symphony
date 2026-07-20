# T2607-02 Local State DB

Status: Migration v1 implemented; projection, query, admin, and integration
slices deferred

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

## Deferred Slices

- `projection`: typed upserts, freshness writes, and active-guard conflict
  mapping;
- `query`: workspace-scoped readers and dashboard snapshots;
- `admin`: health, explicit rebuild/replace, compact, recovery, and manual
  checkpoint operations;
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
versions, unversioned drift, malformed files, bounded contention, and atomic
rollback. Tests inject all paths and do not touch the operator database or the
canonical worktree.
