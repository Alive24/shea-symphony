# T2607-07 App Integration

Design Status: Draft

## Purpose

Make the App the primary operator surface over Temporal and SQLite without
letting it become a second workflow engine.

The App displays operational state, starts or repairs executable work through
the Tauri backend, opens routed Codex/operator flows for human actions, and
reads dashboard/detail snapshots. It does not directly write tracker state,
edit worktrees, run agents, or encode lane policy in UI code.

## Inputs

This package implements decisions from:

- `SNAPSHOT-AND-DASHBOARD.md`;
- `OPERATOR-ACTION-BRIDGE.md`;
- `APP-CLI-SPLIT.md`;
- `WORKFLOW-ACTIVATION.md`;
- `LOCAL-STATE-DB.md`;
- `ISSUE-WORKFLOW-STATE.md`;
- `T2607-03-workflow-coordinator.md`;
- `T2607-06-issue-workflow-state-machine.md`.

## Goals

- Route App operations through a narrow Tauri backend command layer.
- Use SQLite local state DB for top-level dashboard snapshots.
- Use Temporal Query for selected issue detail when an active Workflow exists.
- Use artifact refs and lazy artifact reads for large detail.
- Let App start or repair executable work through the Workflow Coordinator.
- Let App request refresh/cache work through Symphony boundaries.
- Route human input, approval, rework, human fix, and doctor handoff to
  Codex/operator flows and the Operator Action Bridge.
- Keep CLI as admin/dev fallback, not product operation surface.
- Avoid an independent Symphony daemon in 2607.

## Non-Goals

- No App-owned tracker mutation.
- No App-owned worktree edit operation.
- No App-owned agent resume/run operation outside Temporal.
- No App-side lane policy engine.
- No hidden workflow mutation during dashboard render.
- No eager artifact body reads in top-level dashboard refresh.
- No full project history browser as the 2607 dashboard target.
- No visual Workflow Graph editor in 2607.

## Expected Code Areas

Recommended package shape:

```text
app/
  src/
    dashboard/
    issue_detail/
    human_todo/
    runtime_health/

src/
  tauri_backend/
    commands.rs
    snapshot.rs
    temporal_client.rs
    local_state.rs
    operator_actions.rs
    runtime_init.rs
```

Names are illustrative. Follow the current Tauri/app structure where it already
has better local conventions.

## Tauri Backend Command Surface

Expose a small command allowlist.

T2607-02 provides the internal `LocalStateAdmin` library seam only. This
T2607-07 slice owns any eventual Tauri command or operator-facing control over
that seam, and must keep a read-only health check distinct from explicit
migration or later recovery commands.

Recommended commands:

```text
get_dashboard_snapshot(filter) -> DashboardSnapshot
get_issue_detail(issue_ref | workflow_id) -> IssueDetailSnapshot
refresh_visible_items(scope) -> RefreshResult
start_issue_workflow(issue_ref, reason) -> CoordinatorStartResult
repair_issue_workflow(issue_ref, reason) -> CoordinatorRepairResult
prepare_operator_action(issue_ref | workflow_id, action_kind) -> OperatorActionContext
open_operator_flow(context_id) -> OpenResult
get_runtime_health() -> RuntimeHealthSnapshot
run_local_state_health_check() -> LocalStateHealth
```

Commands should call:

- `LocalStateReader` for dashboard reads;
- Temporal Query for one active issue detail;
- Workflow Coordinator for targeted starts/repairs;
- Temporal Update/Signal only through approved Symphony interfaces;
- `LocalStateAdmin` for local DB health/migration/rebuild operations;
- local artifact readers only after drill-down.

Do not expose:

- raw tracker client;
- raw SQLite writer;
- raw Temporal client;
- arbitrary workflow mutation API;
- direct agent-run command;
- direct worktree mutation command.

## Dashboard Snapshot

Top-level dashboard reads should prefer SQLite:

```text
LocalStateReader.get_dashboard_snapshot(filter) -> DashboardSnapshot
```

Dashboard may show:

- current operational lane items;
- human todo items across `Need to Clarify`, `Need Human Input`, and
  `Human Review`;
- current tracker state;
- active/terminal Workflow summary;
- concise PR number/state;
- freshness/staleness;
- runtime health summary;
- artifact ref counts and latest high-signal refs.

Dashboard should not load:

- full transcripts;
- full review reports;
- full diffs;
- full worktree status;
- full tracker history;
- broad historical tracker queues.

If a row is stale, the dashboard may offer explicit refresh. Rendering itself
should not mutate workflow or tracker state.

## Issue Detail

Issue detail reads should combine:

- Temporal Query for active/current Workflow runtime state;
- SQLite artifact index metadata;
- local artifact body reads only after explicit drill-down;
- targeted tracker reads only through explicit refresh or detail action.

Recommended shape:

```text
IssueDetailSnapshot {
  issue_ref
  workflow_id?
  run_id?
  tracker_state
  active_step?
  waiting?
  attempts
  layered_heartbeats
  pr_summary?
  last_transition?
  artifact_refs
  runtime_health
  freshness
}
```

If no active Workflow exists, detail can still show tracker cache and recent
workflow/artifact index rows from SQLite, with freshness markers.

## Refresh Semantics

Refresh must be explicit and bounded.

Allowed refresh actions:

- refresh visible tracker cache rows;
- refresh visible PR summaries;
- refresh artifact index for selected issue;
- run targeted Coordinator repair for selected issue;
- run App-start bounded repair pass.

Refresh should not:

- scan the entire tracker project on every render;
- start workflows for static lanes by default;
- write tracker state directly;
- run agents;
- open worktrees as an imperative workflow action.

Use Temporal Update when the UI needs accepted/rejected feedback. Use
Activities for actual tracker/cache/artifact refresh work.

## App Start

App startup may perform bounded runtime initialization:

- verify local Temporal service availability;
- start or connect local Temporal service when configured;
- verify Symphony workers are polling;
- verify SQLite schema and local state health;
- load workspace/repo config;
- run one bounded Coordinator repair pass for visible/configured executable
  items.

App startup should not become a daemon. After initialization, work proceeds via
explicit user actions, Temporal workflows, and configured worker capacity.

## Human Todo And Operator Actions

The App may display human todo items and action buttons, but it should not
implement the business semantics of human review, rework, input, or doctor
handoff.

Flow:

```text
human todo row
  -> prepare_operator_action(...)
  -> create OperatorActionContext
  -> open Codex/operator flow
  -> routed flow calls submit_operator_action tool/MCP
  -> bridge sends Temporal Update
  -> IssueWorkflow validates and routes
  -> Activities commit tracker/read-model changes
```

Supported operator actions:

- `submit_human_input`;
- `approve_human_review`;
- `request_rework`;
- `submit_human_fix`;
- `doctor_handoff_result`.

CLI shell commands should not be the normal submit path for Coding Agent
operator actions.

## Runtime Health

The App should expose health at useful layers:

- Temporal local service reachable;
- `symphony-core` worker polling;
- `symphony-agent` worker polling or intentionally disabled;
- `symphony-local` worker polling;
- SQLite schema/current health;
- local artifact root readable/writable;
- GitHub/tracker auth available;
- Codex app-server adapter available when agent work is enabled.

Health is read-only unless the user explicitly runs a repair/init command.

## Performance Expectations

Dashboard render should be cheap:

- read SQLite snapshot;
- avoid tracker calls;
- avoid Temporal fanout over every workflow;
- avoid artifact body reads;
- avoid worktree scans;
- expose freshness when data is stale.

Issue detail may pay more cost after drill-down but should still lazy-load
large artifact bodies.

Measure waits separately:

- SQLite read/write;
- Temporal query/start/update;
- tracker read/write;
- artifact read/index;
- Codex app-server queue/session;
- agent execution.

## CLI Boundary

CLI may remain for:

- init when App is unavailable;
- worker run in development/CI;
- local doctor/self-check;
- admin/debug wrappers over Temporal APIs;
- emergency local-state health/rebuild.

CLI must not own product workflow semantics:

- no autopilot loop;
- no lane-loop product runner;
- no direct merge/review/doctor mutation;
- no direct tracker transition command as normal operation.

## Migration Steps

### APP-1: Backend Command Allowlist

- Define Tauri command DTOs.
- Implement read-only snapshot/detail commands.
- Implement Coordinator start/repair command wrappers.
- Block raw tracker/SQLite/Temporal exposure.

### APP-2: Dashboard Snapshot

- Read dashboard from `LocalStateReader`.
- Show human todo summary across all human-facing wait states.
- Show PR summary, freshness, runtime health, and active step.
- Avoid eager artifact reads.

### APP-3: Issue Detail

- Read active issue detail through Temporal Query.
- Combine artifact index refs from SQLite.
- Lazy-load artifact bodies after user drill-down.
- Display layered heartbeats and activity summaries.

### APP-4: Operator Action Routing

- Implement `prepare_operator_action`.
- Create local `OperatorActionContext`.
- Open Codex/operator flow.
- Ensure routed flow submits through tool/MCP bridge.
- Do not implement human-review/rework policy in App UI code.

### APP-5: Runtime Health And Refresh

- Implement runtime health snapshot.
- Implement bounded refresh for visible items.
- Implement App-start bounded repair pass.
- Keep refresh explicit and non-mutating unless the command clearly requests
  refresh/repair.

### APP-6: CLI Reduction

- Mark old product CLI commands legacy-to-delete. If needed, add thin 2607
  admin/debug wrappers over Temporal. They may reuse bounded shared Rust types
  and helpers, but must not rewire the old command graph or workflow ownership.
- Verify normal product operation no longer depends on CLI shell commands.

## Acceptance Checks

- Dashboard snapshot is served from SQLite local state by default.
- Dashboard render does not mutate workflow or tracker state.
- Issue detail uses Temporal Query plus artifact refs.
- Artifact bodies are lazy-loaded, not pulled into top-level dashboard.
- App start can verify runtime health and run bounded repair.
- Operator actions route through `OperatorActionContext` and bridge submission.
- App cannot directly run agents, edit worktrees, merge PRs, link PRs, or write
  tracker state.
- CLI is not required for normal Coding Agent operator action submission.
- No independent Symphony daemon is introduced.

## Done Means

- Tauri backend command allowlist exists;
- dashboard reads SQLite snapshot;
- issue detail reads Temporal Query and artifact refs;
- human todo actions open routed operator flows;
- runtime health is layered and visible;
- refresh semantics are explicit and bounded;
- App and CLI direct mutation bypasses are removed or blocked.
