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

## App Must Not

- directly mutate tracker state;
- directly edit worktrees;
- bypass Temporal workflow and Activity boundaries;
- perform hidden write operations during refresh.

## CLI May

- initialize local config when App is unavailable;
- run local doctor/self-checks;
- run the Symphony worker for development or CI;
- provide thin admin/debug wrappers.

## CLI Must Not

- own product workflow semantics;
- run tick/autopilot loops;
- directly merge, review, doctor, or transition issues as business logic;
- become a second operation surface beside Temporal.

## First App Target

Read-only dashboard and workflow visualization:

- current operational lane items;
- human todo items;
- concise PR number/status;
- current issue state;
- state-grouped workflow steps;
- blocked/needs-input markers that already map to tracker/workflow state;
- latest evidence links, without eager artifact reads.

Manual graph editing belongs to 2608 Workflow Graph Extension or later.

Worktree path, branch name, full trace detail, and artifact bodies belong in
lane item detail, not top-level dashboard refresh.
