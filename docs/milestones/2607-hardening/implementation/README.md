# 2607 Implementation Packages

Status: Draft

## Purpose

This directory translates the 2607 hardening architecture into implementation
packages.

These files are not GitHub issues yet. Promote a package or subpackage into the
tracker only when it has a concrete owner, branch/worktree, and acceptance
criteria ready for the normal Shea Symphony workflow.

## Package Order

Recommended order:

1. `T2607-01-temporal-runtime-skeleton.md`
2. `T2607-02-local-state-db.md`
3. `T2607-03-workflow-coordinator.md`
4. `T2607-04-tracker-transition-activity.md`
5. `T2607-05-agent-activity-boundary.md`
6. `T2607-06-issue-workflow-state-machine.md`
7. `T2607-07-app-integration.md`
8. `T2607-08-deletion-performance-hardening.md`

The order is dependency-aware, not a strict delivery schedule. Packages may run
in parallel when their ownership boundaries are already fixed.

## Dependency Shape

```text
T2607-01 Temporal Runtime Skeleton
  -> provides local Temporal, workers, task queues, skeleton workflow/activity

T2607-02 Local State DB
  -> provides SQLite read model, active workflow index, artifact/tracker cache

T2607-03 Workflow Coordinator
  -> uses Temporal client and SQLite active guard to start executable pulses

T2607-04 TrackerTransitionActivity
  -> provides durable tracker transition and PR-link write boundary

T2607-05 Agent Activity Boundary
  -> provides coarse Main/Rework/Review/Merge/Doctor attempt boundaries

T2607-06 IssueWorkflow State Machine
  -> composes tracker and agent Activities into executable pulse orchestration

T2607-07 App Integration
  -> exposes App/Tauri reads and operations over Temporal and SQLite

T2607-08 Deletion And Performance Hardening
  -> removes old runtime paths and instruments non-LLM control-plane work
```

## Parallel Work Guidance

Can likely proceed in parallel after DTO boundaries are stable:

- `T2607-02` and `T2607-04`;
- `T2607-04` and `T2607-05`;
- `T2607-07` read-only dashboard work and `T2607-02`;
- `T2607-08` inventory work and all earlier packages.

Should not proceed independently:

- `T2607-03` start behavior before `workflow_id` and SQLite guard rules are
  stable;
- `T2607-06` final routing before `T2607-04` and `T2607-05` outcome contracts
  are stable;
- App mutation actions before Operator Action Bridge and Workflow Update
  validation are stable.

## Cross-Package Invariants

These rules apply to every package:

- Tracker writes go through `TrackerTransitionActivity`.
- Workflow progression belongs to `IssueWorkflow`.
- Workflow starts belong to the Coordinator.
- Top-level dashboard reads use SQLite materialized state.
- Selected issue runtime detail uses Temporal Query.
- Agent work uses coarse Activities, not per-model-turn Workflow steps.
- Large evidence lives in artifacts and is referenced by id/path.
- App/Tauri does not directly write tracker, edit worktrees, or run agents.
- CLI is admin/dev fallback only.
- No independent Symphony daemon is introduced in 2607.
- No full Workflow Graph runtime is implemented in 2607.

## Promotion Checklist

Before promoting a package into tracker issues, verify:

- the issue contract names the owning package;
- acceptance checks are copied or narrowed from the package doc;
- no issue reintroduces a deleted boundary such as direct tracker writes or
  App-owned workflow policy;
- dependencies on earlier packages are explicit;
- tests or static checks exist for direct-write and old-loop prevention when
  feasible;
- rollback/recovery behavior is named for external writes;
- artifact and traceability expectations are named.

## Completion Shape

2607 hardening is complete when:

- local Temporal is the only durable runtime spine;
- SQLite is the local read model/cache/index, not workflow truth;
- tracker writes are durable, retryable, and readback-verified Activities;
- agent work is behind typed coarse Activities with capability profiles;
- `IssueWorkflow` owns executable pulse orchestration;
- App reads and operations go through Tauri backend boundaries;
- old autopilot/tick/resume and direct mutation paths are deleted or blocked;
- non-LLM control-plane waits are measured and attributable.
