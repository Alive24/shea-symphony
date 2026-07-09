# Implementation Backlog

Status: Draft

## Purpose

Translate the 2607 hardening architecture decisions into implementation work
packages.

This is not a GitHub issue list yet. Promote work into tracker issues only
when the package has an owner, a concrete target branch, and enough acceptance
criteria to run through the normal Shea Symphony workflow.

## Sequencing Principle

Build the durable runtime spine before adding product surface.

Recommended order:

1. Temporal runtime skeleton
2. Local state DB
3. Workflow Coordinator
4. TrackerTransitionActivity
5. Agent Activity boundary
6. IssueWorkflow state machine
7. App integration
8. Deletion and performance hardening

Packages may run in parallel when their boundaries are stable. Do not parallel
work that would require two implementations to own the same tracker write,
workflow start, worktree lease, or App state interpretation.

## T2607-01 Temporal Runtime Skeleton

Goal:

- introduce local Temporal as the Symphony runtime spine;
- register workers, task queues, and empty workflow/activity shells.

Scope:

- local Temporal startup/dev command;
- `symphony-core`, `symphony-agent`, and `symphony-local` worker registration;
- empty `IssueWorkflow` skeleton with compile-time DTOs;
- empty Activity registrations for tracker, agent, local state, worktree, and
  artifact work;
- Tauri backend Temporal client boundary;
- CLI debug/admin fallback only.

Acceptance:

- a local worker can start and poll all three queues;
- a no-op `IssueWorkflow` can start, query, and complete locally;
- App/Tauri can reach the Temporal client boundary without owning workflow
  semantics;
- old autopilot/tick loop is marked legacy-to-delete in code comments or
  compatibility shims.

Primary docs:

- `implementation/T2607-01-temporal-runtime-skeleton.md`
- `TEMPORAL-SPINE.md`
- `TASK-QUEUES.md`
- `TEMPORAL-RUST-SDK-INTAKE.md`
- `APP-CLI-SPLIT.md`

## T2607-02 Local State DB

Goal:

- create the SQLite local read model/cache/index.

Scope:

- `~/.shea/state/symphony.db` location and override handling;
- built-in schema versioning and minimal migrations;
- typed access layer, no ORM;
- `workflow_index`;
- `tracker_cache`;
- `activity_progress`;
- `artifact_index`;
- `meta`;
- rebuild/admin path for local state.

Acceptance:

- schema initializes idempotently;
- `workflow_index` can enforce one active workflow row per issue for active
  statuses;
- dashboard-style reads can be served without hitting tracker or Temporal
  history for every row;
- SQLite remains rebuildable and cannot authorize workflow progression.

Primary docs:

- `implementation/T2607-02-local-state-db.md`
- `LOCAL-STATE-DB.md`
- `SNAPSHOT-AND-DASHBOARD.md`
- `PERFORMANCE.md`

## T2607-03 Workflow Coordinator

Goal:

- implement the thin launcher/registrar for executable tracker states.

Scope:

- executable tracker state discovery;
- human-readable `workflow_id` construction;
- Temporal `run_id` capture;
- optimistic start with `workflow_index` local guard;
- targeted repair for a single issue;
- App-start repair pass;
- refresh/snapshot lightweight repair for visible issues.

Acceptance:

- Coordinator starts only executable states;
- static states do not create live Workflow executions by default;
- duplicate starts are blocked by local active index and Temporal visibility;
- repair matrix handles `stale_start`, `stale_missing`, closed executions, and
  missing local rows;
- Coordinator does not run agents, choose business transitions, or write
  tracker state.

Primary docs:

- `WORKFLOW-ACTIVATION.md`
- `TEMPORAL-CONCURRENCY.md`
- `LOCAL-STATE-DB.md`

## T2607-04 TrackerTransitionActivity

Goal:

- make Symphony the sole owner of tracker state writes.

Scope:

- `TrackerTransitionRequest` and `TrackerTransitionResult`;
- `expected_from_state` precondition;
- idempotency key;
- readback verification;
- transition evidence refs;
- PR-to-issue link mutation;
- recovery marker migration;
- claim/Project field diet;
- external tracker change conflict handling.

Acceptance:

- no lane, App command, CLI command, or extension writes tracker state
  directly;
- state transitions and PR links are durable, retryable, and observable;
- readback is required before success;
- conflict outcomes route through `IssueWorkflow` rather than hidden mutation
  paths.

Primary docs:

- `TRACKER-TRANSITION-ACTIVITY.md`
- `TRACKER-TRANSITIONS.md`
- `ACTIVITY-ERROR-TAXONOMY.md`

## T2607-05 Agent Activity Boundary

Goal:

- move coding, review, merge, and doctor attempts behind typed coarse
  Activities.

Scope:

- `AgentActivityRequest` and `AgentActivityResult`;
- capability profile enum;
- worktree lease DTO and lifecycle hooks;
- layered heartbeat summaries;
- layered timeout policy;
- cancellation result;
- Codex app-server adapter;
- Agent Review safe-autofix configuration;
- Doctor safe-write policy.

Acceptance:

- Temporal models attempt boundaries, not model turns;
- large transcripts, diffs, test output, and review reports are artifact refs;
- write-capable Activities use worktree leases;
- Agent outputs propose next state but cannot commit tracker transitions;
- automatic doctor can perform only bounded safe repairs and routes uncertain
  repairs to `Need Human Input`.

Primary docs:

- `AGENT-ACTIVITY-CONTRACT.md`
- `RUNTIME-ROLE-MAPPING.md`
- `CHILD-WORKFLOW-POLICY.md`

## T2607-06 IssueWorkflow State Machine

Goal:

- implement `IssueWorkflow` as the executable pulse owner.

Scope:

- `IssueWorkflowInput`;
- durable state DTO;
- terminal outcome enum;
- executable lane handlers;
- internal chaining between executable handlers;
- static handoff and `Done` completion;
- `Need Human Input` handling;
- Activity outcome routing and retry decisions;
- Temporal Query responses for issue detail.

Acceptance:

- Workflow can start from any executable lane;
- executable handlers are independently startable and internally chainable;
- chaining is not exposed as a terminal outcome;
- terminal outcomes are `completed_static_handoff`, `completed_done`,
  `failed_unhandled_error`, and `cancelled`;
- tracker transitions use `TrackerTransitionActivity` with preconditions.

Primary docs:

- `ISSUE-WORKFLOW.md`
- `ISSUE-WORKFLOW-STATE.md`
- `WORKFLOW-ACTIVATION.md`
- `ACTIVITY-ERROR-TAXONOMY.md`

## T2607-07 App Integration

Goal:

- make App the product operation surface over Temporal and SQLite.

Scope:

- App start repair trigger;
- dashboard snapshot from SQLite;
- issue detail from Temporal Query plus artifact refs;
- snapshot/refresh commands through Tauri backend;
- Operator Action Bridge to Temporal Update;
- open-in-Codex/operator flow context;
- no direct App tracker writes;
- no independent Symphony daemon.

Acceptance:

- dashboard refresh does not mutate workflow state;
- issue detail can show active Workflow, Activity, heartbeat, artifact, and
  tracker cache summaries;
- operator actions route through a typed bridge and are revalidated by
  Workflow;
- App cannot directly run agents or mutate tracker state.

Primary docs:

- `SNAPSHOT-AND-DASHBOARD.md`
- `OPERATOR-ACTION-BRIDGE.md`
- `APP-CLI-SPLIT.md`
- `WORKFLOW-ACTIVATION.md`

## T2607-08 Deletion And Performance Hardening

Goal:

- remove old runtime paths after Temporal-backed paths work.

Scope:

- delete or quarantine old autopilot/tick/resume loop;
- remove duplicate tracker reads;
- remove direct tracker write paths;
- remove dashboard command churn;
- replace vendored runtime assumptions;
- add timing/measurement points for non-LLM paths.

Acceptance:

- old lane mutation path is not a second durable runtime;
- no production App path depends on hidden source-of-truth state;
- non-LLM control-plane operations are seconds-scale unless blocked by
  external services;
- performance measurements identify tracker, SQLite, Temporal, artifact, and
  agent-backend waits separately.

Primary docs:

- `SUBTRACTION-INVENTORY.md`
- `PERFORMANCE.md`
- `ROADMAP.md`

## Deferred To 2608

- full Workflow Graph runtime;
- extension module loader;
- visual Workflow Graph editor;
- default Child Workflow decomposition of core lanes;
- Linear integration.
