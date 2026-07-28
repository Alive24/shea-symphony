# T2607-01 Temporal Runtime Skeleton

Status: Draft

## Purpose

Create the minimum local Temporal runtime skeleton that later 2607 work can
attach to.

This package should not implement real tracker transitions, agent execution,
SQLite projection, or App product behavior. It establishes the runtime shape,
typed DTO locations, worker registration, task queues, and client boundary.

## Goal

- local Temporal can be reached from the Symphony runtime;
- workers register the starting task queues;
- a no-op `IssueWorkflow` can start, query, and complete;
- no-op Activities are registered under the intended names;
- App/Tauri reuses the Symphony-owned Temporal client boundary without owning
  workflow semantics;
- CLI wrappers remain optional admin/dev follow-up work rather than a package
  completion requirement.

## Non-Goals

- No real tracker writes.
- No real agent/Codex execution.
- No real SQLite projection.
- No Workflow Coordinator implementation.
- No production dashboard rewrite.
- No Child Workflow decomposition.
- No independent Symphony daemon.
- No Temporal Cloud dependency.

## Expected Code Areas

Exact paths may change after code inspection, but the first implementation
should aim for these ownership boundaries:

- `src/main.rs` as the default 2607 Temporal worker runtime entrypoint;
- `src/symphony/**` for Temporal client, workers, DTOs, and
  workflow/activity registration;
- App/Tauri backend command layer for bounded, read-only Temporal readiness
  through the active workspace configuration;
- test/support utilities for local Temporal integration checks.

Do not create a separate `temporal_runtime` product namespace unless codebase
constraints force it. Temporal is the runtime spine inside `symphony`.

## Entrypoint Decision

2607 intentionally moves the default binary away from the MVP CLI dispatcher.
`src/main.rs` should start the Symphony Temporal worker runtime directly. The
old CLI/autopilot entrypoint is retained as context through git history, the
protected `2606-MVP` branch, and legacy modules that have not yet been
mechanically removed.

Do not add new 2607 runtime behavior to the old CLI dispatcher or lane loop as
a compatibility measure. If a future issue needs an operator/admin surface, it
should go through App/Tauri, a Temporal Query/Signal/Update boundary, or a
deliberately scoped admin surface documented by that issue.

The broader ownership rule is recorded in
`docs/milestones/2607-hardening/CODE-OWNERSHIP-MAP.md`.

## DTO Skeleton

Define compile-time DTOs even if the first workflow is no-op.

Minimum workflow DTOs:

```text
IssueWorkflowInput {
  workflow_id
  repo_id
  issue_ref
  from_tracker_state
  target_kind
  source_ref
  source_tracker_revision
  started_at
  operator_action_ref?
  capacity_policy_ref?
}

IssueWorkflowState {
  workflow_id
  run_id?
  repo_id
  issue_ref
  current_tracker_state
  active_step
  terminal_outcome?
  artifact_refs
  runtime_health_summary
}

IssueWorkflowQueryResult {
  workflow_id
  run_id?
  issue_ref
  current_tracker_state
  active_step
  terminal_outcome?
  runtime_health_summary
  artifact_refs
}
```

Minimum Activity DTOs:

```text
NoopActivityRequest {
  workflow_id
  activity_kind
  issue_ref
}

NoopActivityResult {
  outcome
  summary
  artifact_refs
}
```

The DTOs should be small and serializable. Do not pass full tracker issues,
large transcripts, diffs, or review reports through Workflow history.

## Task Queues

Register workers for:

- `symphony-core`
- `symphony-agent`
- `symphony-local`

Initial registration:

```text
symphony-core:
  workflows:
    IssueWorkflow
  activities:
    NoopCoreActivity
    TrackerTransitionActivity placeholder

symphony-agent:
  activities:
    MainAgentActivity placeholder
    ReworkActivity placeholder
    AgentReviewActivity placeholder
    MergeActivity placeholder

symphony-local:
  activities:
    LocalStateProjectionActivity placeholder
    ArtifactIndexActivity placeholder
    LocalHealthActivity placeholder
```

Placeholders should return typed `not_implemented` or `noop_success` outcomes
that are safe for tests. They must not mutate tracker, worktree, SQLite, or
artifacts.

## No-Op IssueWorkflow Behavior

The first `IssueWorkflow` should:

1. accept `IssueWorkflowInput`;
2. initialize small durable state;
3. expose a query for current state;
4. optionally call one no-op core Activity;
5. complete with a no-op terminal result.

It should not:

- scan tracker;
- create worktrees;
- start agents;
- write SQLite;
- transition tracker state;
- infer next lane behavior.

The point is proving the Temporal runtime path, not proving the business
workflow.

## Temporal Client Boundary

Create one Symphony-owned Temporal client boundary used by App/Tauri. Optional
CLI admin/dev wrappers may reuse the same boundary later, but are not required
to complete T2607-01.

Required operations for this package:

- connect to local Temporal;
- start a no-op `IssueWorkflow`;
- query a running/completed no-op `IssueWorkflow`;
- check worker/task queue health where possible;
- return typed errors when Temporal is unavailable.

The App calls a no-argument Tauri readiness command that snapshots the active
workspace, resolves that workspace's exact workflow file, validates the shared
runtime configuration, and calls `SymphonyTemporalClient::check_service()`.
The service check is bounded to five seconds and returns the captured
workspace/workflow identity with `ready`, `unavailable`, `timedOut`, or
`invalidConfig`; it does not start or control a Workflow. The App must not
construct task queue names, raw payloads, or workflow semantics.

CLI may expose thin admin/dev wrappers over the same boundary in a later,
explicitly scoped issue.

## Local Temporal Startup Strategy

2607 is local-first and does not depend on Temporal Cloud.

The supported developer path is the explicitly gated,
repo-owned [`TEMPORAL-NOOP-SMOKE.md`](../TEMPORAL-NOOP-SMOKE.md) command. It
probes the configured local endpoint, refuses to touch an already-running
service, and otherwise starts a test-owned headless Temporal CLI dev server
with bounded readiness retries. It reports a typed unavailable-service error
when the endpoint cannot be reached and does not hide startup failures behind
agent or tracker errors.

The worker and smoke both select `.shea/workflows/shea-symphony.md`; the 2607
worker entrypoint defaults to that checked-in profile rather than the removed
legacy `workflows/shea-symphony.md` path.

## Configuration

Minimum config keys:

```text
temporal.address
temporal.namespace
temporal.task_queues.core
temporal.task_queues.agent
temporal.task_queues.local
temporal.worker.core_concurrency
temporal.worker.agent_concurrency
temporal.worker.local_concurrency
```

Defaults should match the 2607 docs:

- core concurrency: 3;
- agent concurrency: 3;
- local concurrency: 8.

## SDK Version Choice

As of the T2607-01 implementation pass on 2026-07-09, the official Temporal
Rust SDK quickstart documents the compatible crate family as:

```text
temporalio-client = "0.5.0"
temporalio-common = "0.5.0"
temporalio-macros = "0.5.0"
temporalio-sdk = "0.5.0"
temporalio-sdk-core = "0.5.0"
```

The skeleton pins that crate family together and keeps SDK-facing types inside
the `symphony` runtime module so later 2607 slices can absorb SDK API drift at
one boundary.

Use the existing workspace config precedence:

1. workspace-local config;
2. repo team shared `.shea` config;
3. global `~/.shea` config.

## Tests And Checks

Minimum acceptance checks:

- compile/typecheck for the new runtime module;
- worker registration test or integration check for all three queues;
- no-op workflow start/query/complete check;
- unavailable Temporal service returns a typed error;
- App/Tauri command can call the runtime boundary without owning payload
  details, and deterministically covers ready, unavailable, invalid-config,
  timeout, active-workspace selection, and stale-response identity behavior;
- any future CLI debug wrapper calls the same runtime boundary.

If local Temporal integration tests are too heavy for default CI, mark them as
explicit integration tests and keep unit tests around DTO construction and
client-boundary error mapping. The no-op smoke is ignored by default and
requires the explicit repo-owned command described in
[`TEMPORAL-NOOP-SMOKE.md`](../TEMPORAL-NOOP-SMOKE.md).

## Bootstrap, Rollback, And Compatibility

The protected 2606 MVP branch remains the active external App/CLI bootstrap,
recovery baseline, and acceptance oracle while the 2607 path is incomplete.

This package should not damage that protected runtime. In current main it should
mark old autopilot/tick/resume paths as inactive legacy-to-delete; it must not
turn them into compatibility shims or runtime dependencies. Deletion belongs in
later implementation packages after Temporal-backed paths work. Bounded Rust
types and helpers may still be reused after extraction and ownership review.

## Done Means

- `IssueWorkflow` exists as a no-op Temporal Workflow.
- Starting task queues are registered.
- Placeholder Activities are registered.
- A local no-op Workflow can start, be queried, and complete.
- The gated local smoke cleans up only the service and worker processes it owns.
- App/Tauri reuses the Symphony runtime client boundary for bounded readiness;
  a CLI wrapper is optional admin/dev follow-up work.
- No real business side effects occur through the skeleton.
