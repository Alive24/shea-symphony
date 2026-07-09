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
- App/Tauri and CLI use the same Temporal client boundary without owning
  workflow semantics.

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

- `symphony` runtime module/crate for Temporal client, workers, DTOs, and
  workflow/activity registration;
- App/Tauri backend command layer for starting local runtime and reaching the
  Temporal client boundary;
- CLI admin/dev fallback for local worker/dev startup only;
- test/support utilities for local Temporal integration checks.

Do not create a separate `temporal_runtime` product namespace unless codebase
constraints force it. Temporal is the runtime spine inside `symphony`.

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

Create one Symphony-owned Temporal client boundary used by App/Tauri and CLI.

Required operations for this package:

- connect to local Temporal;
- start a no-op `IssueWorkflow`;
- query a running/completed no-op `IssueWorkflow`;
- check worker/task queue health where possible;
- return typed errors when Temporal is unavailable.

The App should call Tauri backend commands that call this boundary. The App
must not construct task queue names, raw payloads, or workflow semantics.

CLI may expose thin admin/dev wrappers over the same boundary.

## Local Temporal Startup Strategy

2607 is local-first and does not depend on Temporal Cloud.

The package should define one supported dev startup path, such as:

- use an existing local Temporal service if reachable;
- provide a documented local dev command or script to start it;
- fail with a clear typed error if no service is reachable.

Do not hide Temporal startup failures behind agent or tracker errors.

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
  details;
- CLI debug wrapper, if present, calls the same runtime boundary.

If local Temporal integration tests are too heavy for default CI, mark them as
explicit integration tests and keep unit tests around DTO construction and
client-boundary error mapping.

## Rollback And Compatibility

The protected 2606 MVP branch remains the fallback.

This package should not delete working MVP runtime behavior. It may add
compatibility shims or mark old autopilot/tick/resume paths as
legacy-to-delete, but deletion belongs in later implementation packages after
Temporal-backed paths work.

## Done Means

- `IssueWorkflow` exists as a no-op Temporal Workflow.
- Starting task queues are registered.
- Placeholder Activities are registered.
- A local no-op Workflow can start, be queried, and complete.
- App/Tauri and CLI share one Symphony runtime client boundary.
- No real business side effects occur through the skeleton.
