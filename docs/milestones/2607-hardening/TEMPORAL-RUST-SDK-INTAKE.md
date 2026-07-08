# Temporal Rust SDK Intake

Status: Draft

## Purpose

Ground 2607 Temporal migration decisions in the current official Temporal Rust
SDK material before defining Activity contracts or Tauri backend commands.

This is a narrow intake, not a broad Temporal study.

## Sources

- Temporal Rust SDK developer guide: https://docs.temporal.io/develop/rust
- Rust SDK quickstart: https://docs.temporal.io/develop/rust/quickstart
- Workflow basics: https://docs.temporal.io/develop/rust/workflows/basics
- Activity basics: https://docs.temporal.io/develop/rust/activities/basics
- Activity execution: https://docs.temporal.io/develop/rust/activities/execution
- Workflow message passing: https://docs.temporal.io/develop/rust/workflows/message-passing
- Worker processes: https://docs.temporal.io/develop/rust/workers/worker-process
- Temporal Client: https://docs.temporal.io/develop/rust/client/temporal-client
- Rust SDK README: https://github.com/temporalio/sdk-rust/blob/main/crates/sdk/README.md

## SDK Status

The Rust SDK is Public Preview and actively evolving. 2607 accepts that because
Shea Symphony is not production-ready and the goal is to replace a hand-rolled
durable loop with Temporal's model.

Design implication:

- keep Temporal-facing payloads small and versionable;
- isolate SDK usage behind the `symphony` runtime boundary;
- avoid spreading SDK types through Shea prompt/skill code.

## Local Development Runtime

The quickstart uses local Temporal service through Temporal CLI:

```text
temporal server start-dev
```

The local service listens on `localhost:7233`; the local Web UI is exposed by
the dev server.

2607 implication:

- Temporal Cloud stays out of scope;
- App first-run can check/start/connect local Temporal service;
- worker startup is local;
- local Temporal Web UI can be linked for traceability.

## Workflow Constraints

Rust Workflows are structs with macro-defined methods:

- `#[init]` initializes Workflow state;
- `#[run]` contains main Workflow logic;
- `#[signal]` handles asynchronous external messages;
- `#[query]` exposes read-only state;
- `#[update]` can mutate state and return a result.

Workflow state is persisted and replayed. Fields and inputs must be serializable.

Workflow logic must be deterministic:

- no filesystem/network/process I/O in Workflow code;
- no direct system time;
- no random generation;
- no direct `tokio` or `futures` concurrency primitives that break replay;
- use Workflow-safe SDK primitives such as timers, wait conditions,
  deterministic select/join wrappers, Activities, and child Workflows.

2607 implication:

- `IssueWorkflow` orchestrates only;
- GitHub, git, Codex, LLM, filesystem, tracker, and artifact writes are
  Activities;
- random ids, wall clock timestamps, and local path probing belong in
  Activities or initial inputs;
- `resume_target`, waiting reason, current state, and artifact refs belong in
  serializable Workflow state.

## Activity Constraints

Activities are async functions/methods marked by `#[activity]`. They perform
I/O and non-deterministic work.

Activity inputs and outputs must be serializable. Official docs recommend one
struct argument to avoid breaking signatures. Payloads are recorded in Workflow
history, with practical limits:

- one argument up to 2 MB;
- total gRPC message up to 4 MB;
- large histories hurt worker performance.

Activities must have timeouts when scheduled. Long-running Activities may use
heartbeats for progress and cancellation behavior.

2607 implication:

- every Shea Activity uses one request struct and one result struct;
- Activity results store summaries and artifact refs, not full transcripts;
- `MainAgentActivity` and merge/review Activities need heartbeat/progress
  design;
- large logs, transcripts, patches, and reports live in local artifact storage;
- retryable vs non-retryable errors must follow
  `ACTIVITY-ERROR-TAXONOMY.md`.

## Message Passing

Queries:

- read Workflow state;
- cannot mutate state;
- cannot perform async operations such as Activities;
- do not add events to Workflow history;
- require a Worker polling the task queue.

Signals:

- can be sent from clients or Workflows;
- only work for open Workflow executions;
- return when the server accepts the Signal, not when the Workflow handles it.

Updates:

- are synchronous, blocking calls that may mutate state and return a result;
- require a Worker to accept/reject and process them;
- accepted/completed Update events are written to Workflow history;
- validators can reject Updates before they are written to history.

2607 implication:

- issue detail refresh uses Query;
- dashboard refresh may use a SQLite materialized read model populated from
  Temporal/tracker/artifact projections;
- fire-and-continue human input can use Signal;
- UI actions that need accepted/rejected feedback should use Update when Rust
  SDK support is adequate;
- if Update ergonomics block migration, use Signal plus Query without changing
  architecture;
- query handlers should never read artifacts directly.

## Worker Constraints

Workers poll a task queue and execute registered Workflow and Activity types.

Important constraints:

- a Worker Entity is associated with one task queue;
- all workers polling the same task queue must register the same Workflow and
  Activity types;
- if a Worker receives a task for an unknown type, that task fails.

2607 implication:

- start with one local `shea-symphony` task queue;
- register `IssueWorkflow` and all core Activities in the same worker;
- do not split task queues until there is a measured reason;
- App initialization must verify that the worker is polling the expected queue.

## Client Constraints

Starting a Workflow requires:

- Workflow type;
- Workflow input;
- task queue;
- Workflow ID.

Workflow ID should map to the business entity.

2607 implication:

- use a stable ID such as `issue:<repo-id>:<issue-number>`;
- Tauri backend owns workflow ID construction;
- App does not expose Temporal task queue or payload details;
- CLI debug wrappers, if any, call the same client boundary.

## Shea Symphony Design Rules

- `IssueWorkflow` is durable orchestration.
- Activities own side effects.
- Tauri backend calls Temporal Client directly.
- CLI is admin/dev fallback only.
- Core runtime code should use the `symphony` naming boundary. Do not create a
  separate `temporal_runtime` package name unless implementation constraints
  force it.
- Temporal history stores state, event order, summaries, retries, and artifact
  refs.
- Local artifact store holds large data.
- SDK types should not leak into Shea prompts, skills, or semantic gates.
- Do not design an independent local Symphony service in 2607.

## Open Checks Before Implementation

- Confirm Rust SDK crate versions in repo dependency policy.
- Verify Update support and ergonomics in a tiny local sample.
- Verify Activity heartbeat API shape for long-running Codex agent work.
- Verify local Temporal dev server persistence options; quickstart uses
  `start-dev`, but 2607 may need durable local data under `~/.shea`.
- Decide whether `Backlog` shaping should be one long-lived `IssueWorkflow` or
  a child workflow if history grows too large.
