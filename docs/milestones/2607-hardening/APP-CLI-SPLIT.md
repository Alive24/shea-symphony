# App And CLI Split

Status: Draft

## Principle

Temporal local runtime is the execution authority. The App is the primary
operator surface. CLI is admin/dev fallback only.

Do not introduce an independent local Symphony service as a 2607 target. The
Tauri backend command layer can call the Temporal client directly.

## App May

- display state-grouped workflow structure;
- display current workflow step and issue state;
- display snapshots;
- display logs, traces, and artifacts after issue-level drill-down;
- initialize local runtime state;
- start or connect local Temporal service and Symphony worker;
- start workflows through the Tauri backend;
- send Temporal signals or updates through the Tauri backend;
- query Temporal-backed snapshots through the Tauri backend;
- show disabled or bypassed workflow steps when available.
- open or route to Codex/operator flows for human input, approval, rework, or
  doctor work.

## App Must Not

- directly mutate tracker state;
- directly edit worktrees;
- bypass Temporal workflow and Activity boundaries;
- implement human review, rework, or human-input business semantics inside UI
  code;
- perform hidden write operations during refresh.

## Operation Routing

Recommended routing:

```text
Dashboard render
  -> LocalStateReader.get_dashboard_snapshot

Issue detail open
  -> Temporal Query for selected workflow
  -> LocalStateReader.list_artifacts for artifact refs

Refresh tracker cache / PR summaries
  -> Tauri backend command
  -> Temporal Update when accepted/rejected feedback matters
  -> Activity refreshes tracker/cache
  -> LocalStateProjector updates SQLite

Start issue workflow
  -> Tauri backend command
  -> Temporal start workflow

Human input / approve / request rework / human fix
  -> App opens Codex/operator flow
  -> Coding Agent or operator flow calls narrow tool/MCP action bridge
  -> Bridge submits Temporal Update to Symphony
  -> Workflow validates the request
  -> Activities update tracker/read model

DB health / explicit migration / later rebuild
  -> T2607-07 Tauri backend command
  -> T2607-02 LocalStateAdmin library
```

T2607-02 owns only the internal `LocalStateAdmin` library boundary. It does
not add a product CLI command, Tauri command, or Temporal Activity. T2607-07
later owns the Tauri command/operator surface and must preserve the distinction
between read-only health inspection and an explicit migration or recovery
request.

The App may expose buttons or links for human todo actions, but those controls
should route to the appropriate Coding Agent/operator flow. They should not
apply tracker changes or encode review/rework policy in the App.

Routed flows should use the Operator Action Bridge tool/MCP interface. Do not
make CLI shell commands the normal submit path for Coding Agent actions.

For refresh operations, prefer Temporal Update when the UI needs immediate
accepted/rejected feedback. The refresh work itself may continue
asynchronously and expose progress through workflow state and SQLite
projection.

## CLI May

- initialize local config when App is unavailable;
- run local doctor/self-checks;
- run the Symphony worker for development or CI;
- provide thin admin/debug wrappers over Temporal start, query, signal, or
  update APIs.
- provide debug-only wrappers for operator action submission if needed.

Until T2607-07 provides an operator surface, LocalStateAdmin remains a library
seam rather than a normal product CLI path.

## CLI Must Not

- own product workflow semantics;
- run tick/autopilot loops;
- directly merge, review, doctor, or transition issues as business logic;
- be the primary submit channel for Coding Agent operator actions;
- become a second operation surface beside Temporal.

Existing product commands such as autopilot, main loop, review loop, merge
loop, and mutating doctor should be deleted. If migration temporarily needs an
admin/debug entrypoint, implement a thin 2607 wrapper over Temporal
Signals/Queries/Updates. Bounded shared Rust types and helpers may be reused,
but the 2606 product-command graph and its workflow ownership must not be
retained or rewired.

## First App Target

Read-only dashboard and workflow visualization:

- current operational lane items;
- human todo items;
- both `Need Human Input` and `Human Review` items in the same human todo
  surface, with the underlying state visible in detail;
- concise PR number/status;
- current issue state;
- state-grouped workflow steps;
- blocked/needs-input markers that already map to tracker/workflow state;
- latest evidence links, without eager artifact reads.

Manual graph editing belongs to 2608 Workflow Graph Extension or later.

Worktree path, branch name, full trace detail, and artifact bodies belong in
lane item detail, not top-level dashboard refresh.
