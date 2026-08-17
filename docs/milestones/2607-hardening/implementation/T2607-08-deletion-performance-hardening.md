# T2607-08 Deletion And Performance Hardening

Design Status: Draft

## Purpose

Remove or quarantine the old runtime paths after Temporal-backed paths exist,
and add enough measurement to prove non-LLM control-plane work is no longer the
dominant delay.

This is the subtraction closeout package for 2607. It should make the system
clearer, faster, and easier to debug before new product surface is added.

## Inputs

This package implements decisions from:

- `SUBTRACTION-INVENTORY.md`;
- `PERFORMANCE.md`;
- `ROADMAP.md`;
- `TEMPORAL-SPINE.md`;
- `APP-CLI-SPLIT.md`;
- `TRACKER-TRANSITION-ACTIVITY.md`;
- `LOCAL-STATE-DB.md`.

## Goals

- Delete or quarantine old autopilot/tick/resume runtime loops.
- Delete or block direct tracker mutation paths outside
  `TrackerTransitionActivity`.
- Remove repeated tracker reads from normal App/workflow paths.
- Replace dashboard command churn with SQLite dashboard snapshots.
- Replace direct product CLI semantics with Temporal admin/debug wrappers.
- Remove vendored runtime assumptions from target repositories.
- Add timing and trace spans for non-LLM control-plane work.
- Make stuck state diagnosable by layer: Temporal, SQLite, tracker, artifact,
  worktree, Codex/app-server, agent backend.

## Non-Goals

- No new user-visible feature expansion.
- No broad visual Workflow Graph editor.
- No new tracker backend.
- No broad LLM/reviewer provider expansion.
- No performance rewrite that leaves ownership unclear.
- No cosmetic file moves without ownership boundary improvement.

## Deletion Categories

### Legacy Runtime Loop

Delete, reduce, or quarantine:

- old autopilot loop;
- tick loop;
- resume loop;
- lane runner that owns durable retry/state;
- custom retry/resume state files that duplicate Temporal history;
- code paths that infer progress from local files instead of Workflow state.

Allowed temporary admin/debug entrypoint:

- a thin 2607 boundary that starts, queries, signals, or updates Temporal. It may
  reuse bounded shared Rust types and helpers, but must not call or wrap the old
  loop or command graph.

Not allowed:

- a second durable scheduler;
- a second retry engine;
- a second source of workflow terminal state.

### Direct Tracker Writes

Delete or block direct writes for:

- lane status changes;
- claim set/clear;
- workpad/timeline evidence append;
- PR-to-issue link mutation;
- terminal issue close;
- merge/land terminal writes;
- mutating doctor repair;
- App/Tauri mutation bypass;
- product CLI mutation commands;
- extension/Coding Agent tracker writes.

Replacement:

- `IssueWorkflow` decides;
- `TrackerTransitionActivity` commits and readback-verifies.

### Repeated Tracker Reads

Remove normal-path reads that exist only because local state was not durable.

Replace with:

- Temporal Query for one active issue's runtime state;
- SQLite dashboard read model for multi-issue views;
- targeted tracker readback only after writes;
- explicit refresh for stale visible rows;
- targeted repair when a conflict is suspected.

Manual tracker edits remain exception paths. Do not make ordinary dashboard
render pay for rare external edits.

### App Command Churn

Remove App paths that:

- shell out to CLI for product state;
- run mutating commands during refresh;
- scan all artifacts for top-level dashboard;
- scan worktrees for every row;
- fan out to Temporal/tracker/filesystem for every render.

Replacement:

- `LocalStateReader.get_dashboard_snapshot`;
- issue-detail Temporal Query;
- lazy artifact reads;
- explicit bounded refresh/repair commands.

### Vendored Runtime Assumptions

Remove assumptions that Symphony runtime lives inside each target repo.

Replacement:

- local install lookup for Symphony binary/runtime;
- tracked repo `.shea/` for team config;
- `~/.shea/` for local state, generated worktrees, artifacts, operator action
  contexts, and workspace-local config.

### Mixed Ownership Files

Split or move files only when the move clarifies ownership:

- Workflow orchestration;
- Activity side effects;
- tracker adapter;
- App read model;
- local state projection;
- agent backend adapter;
- extension/Shea semantics.

Avoid broad movement that only changes names.

## Quarantine Rules

If old code cannot be deleted immediately, quarantine it as inactive reference:

- put it behind a clearly named `legacy` or `compat` module;
- prevent new Temporal, Activity, Tauri, CLI, and operator product paths from
  calling it;
- add comments naming the replacement package;
- add tests or grep checks proving product paths do not call it directly;
- track deletion in this milestone docs or a follow-up tracker issue when
  implementation begins.

If operator access needs a temporary shim, implement a new thin 2607
start/query/signal/update entrypoint outside the old product implementation.
The protected 2606 App/CLI remains the external bootstrap until that entrypoint
is ready. Selective reuse of extracted Rust components is allowed, but 2607
does not turn the old runtime or product command graph into an alternate or
compatibility runtime.

## Measurement Model

Add timing spans around non-LLM control-plane work.

Recommended span dimensions:

```text
TimingSpan {
  span_id
  workflow_id?
  issue_ref?
  operation
  layer
  started_at
  finished_at
  duration_ms
  outcome
  retry_count?
  wait_reason?
  artifact_refs
}
```

Initial layers:

- `temporal`;
- `sqlite`;
- `tracker`;
- `artifact`;
- `worktree`;
- `app_backend`;
- `codex_app_server`;
- `agent_backend`;
- `github_cli`;
- `local_process`.

Keep timing payloads small. Detailed logs belong in artifacts.

## Required Measurement Points

### App/Dashboard

- dashboard snapshot read;
- visible refresh command;
- issue detail Query;
- artifact index lookup;
- artifact body lazy load;
- runtime health check.

### Temporal

- workflow start;
- workflow query;
- signal/update accepted/rejected latency;
- Activity schedule-to-start delay;
- Activity duration;
- Activity retry count;
- durable timer/wait duration.

### Tracker

- targeted issue read;
- Project field read/write;
- tracker transition write;
- tracker transition readback;
- PR-to-issue link write/readback;
- tracker cache refresh.

### SQLite

- dashboard query;
- workflow index guard insert/update;
- tracker cache projection;
- artifact index projection;
- health check;
- rebuild/compact.

### Agent/Worktree

- worktree lease acquire/release;
- worktree status summary;
- Codex app-server session queue;
- Codex app-server accepted/running latency;
- agent attempt duration;
- merge/readback duration.

## Performance Acceptance Bias

Use measurements to answer:

- are we waiting on LLM/agent work or local orchestration?
- are App refreshes reading tracker, artifacts, or worktrees unnecessarily?
- are tracker writes slow because of write/readback or repeated setup?
- are SQLite reads fast enough for dashboard?
- are Temporal Queries fast enough for issue detail?
- are agent Activities stuck in Temporal scheduling, Codex queue, or model
  progress?

Initial qualitative target:

- non-LLM local control-plane operations should be seconds-scale unless waiting
  on an external service;
- dashboard render should be cheap enough that it does not feel blocked after
  LLM work completed;
- status snapshots should name the slow layer.

Do not lock hard millisecond budgets until the first timing pass exists.

## Stuck-State Diagnosis

Every stuck item should have a layer and owner:

- `temporal`: workflow/activity scheduling, retry, timer, cancellation;
- `sqlite`: projection stale/failed, schema issue, rebuild needed;
- `tracker`: read/write/readback/rate limit/auth/schema conflict;
- `artifact`: missing/failed large evidence write or read;
- `worktree`: lease conflict, dirty unsafe state, missing checkout;
- `codex_app_server`: queued, session failed, transport issue, usage limit;
- `agent_backend`: no progress, invalid result, model/tool failure;
- `operator`: waiting for human action.

If a human action is required, the tracker state should reflect a human-facing
wait such as `Need Human Input`, `Need to Clarify`, or `Human Review`, rather
than leaving the item stuck only in local UI text.

## Guard Checks

Add automated checks where cheap:

- no production App path imports raw tracker mutation APIs;
- no product CLI command writes tracker state directly;
- no agent Activity can call tracker transition write functions except through
  `TrackerTransitionActivity`;
- no dashboard render path performs artifact body reads;
- no dashboard render path scans worktrees;
- no Workflow Query handler performs I/O;
- old autopilot/tick/resume entrypoints are absent or marked legacy-to-delete.

Use grep/static checks where full tests are not yet available. Replace them
with stronger tests as implementation matures.

## Migration Steps

### DPH-1: Inventory Concrete Paths

- List old autopilot/tick/resume entrypoints.
- List direct tracker write functions and callers.
- List App refresh commands and data sources.
- List CLI product commands.
- List vendored runtime assumptions.

### DPH-2: Add Timing Spans

- Add lightweight timing helper.
- Add spans to dashboard, Temporal, tracker, SQLite, artifact, worktree, and
  agent boundaries.
- Expose timing summaries in issue detail or runtime health snapshots.

### DPH-3: Delete Or Quarantine Old Runtime

- Remove old loop entrypoints after Temporal equivalents exist.
- Quarantine temporary shims behind Temporal boundaries.
- Add guard checks preventing product paths from calling legacy loops.

### DPH-4: Delete Direct Mutation Paths

- Move remaining writes to `TrackerTransitionActivity`.
- Remove App/CLI/lane direct mutation calls.
- Verify PR link and terminal writes are inside durable Activity boundary.

### DPH-5: Read Path Diet

- Move dashboard reads to SQLite snapshots.
- Move issue detail to Temporal Query plus artifact refs.
- Remove eager artifact and worktree reads from top-level dashboard.
- Use explicit refresh for stale visible data.

### DPH-6: Workspace Runtime Cleanup

- Remove vendored runtime dependency from target repo assumptions.
- Verify `~/.shea/` owns local state, generated worktrees, artifacts, and
  operator-action contexts.
- Verify repo `.shea/` remains tracked team config.

## Acceptance Checks

- Old autopilot/tick/resume loop is deleted or unable to act as a second
  durable runtime.
- No normal App path directly mutates tracker state.
- No product CLI path directly owns workflow product semantics.
- No agent/review/doctor path bypasses capability or tracker boundaries.
- Dashboard render uses SQLite snapshot and lazy artifact reads.
- Issue detail uses Temporal Query plus artifact refs.
- Tracker writes are readback-verified through
  `TrackerTransitionActivity`.
- Timing spans identify Temporal, SQLite, tracker, artifact, worktree, App
  backend, Codex app-server, and agent-backend waits separately.
- Stuck states expose a layer, reason, owner, and recommended next action.
- Vendored runtime assumptions are removed from target repo workflow.

## Done Means

- old runtime paths are deleted or quarantined behind Temporal;
- direct write bypasses are deleted or blocked;
- dashboard and issue-detail read paths follow the 2607 read model;
- timing instrumentation exists at key non-LLM boundaries;
- stuck state diagnosis is structured enough to guide operator or developer
  action;
- 2607 can proceed to implementation issues without unresolved second-runtime
  ambiguity.
