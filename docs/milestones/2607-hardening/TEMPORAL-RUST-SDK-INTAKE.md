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

The upstream quickstart uses a local Temporal service through the Temporal CLI.
For this repository, T2607-01 defines exactly one supported local startup and
runtime-proof path:

[`TEMPORAL-NOOP-SMOKE.md`](TEMPORAL-NOOP-SMOKE.md).

That explicitly gated command owns a transient `localhost:7233` service only
when the endpoint is unavailable, invokes the supported headless CLI form, and
cleans it up. Do not add a separate raw CLI, Docker, or Temporal Cloud startup
path for this test slice. A future App-owned runtime path remains out of scope.

2607 implication:

- Temporal Cloud stays out of scope;
- T2607-01 owns only the smoke harness; App first-run is deferred;
- worker startup is local;
- the smoke does not rely on or link to the local Temporal Web UI.

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
- fire-and-continue routed agent/operator results can use Signal;
- routed agent/operator actions that need accepted/rejected feedback should use
  Update when Rust SDK support is adequate;
- if Update ergonomics block migration, use Signal plus Query without changing
  architecture;
- App UI should open Codex/operator flows for human input, approval, human fix,
  and rework actions instead of implementing those semantics directly;
- query handlers should never read artifacts directly.

## Worker Constraints

Workers poll a task queue and execute registered Workflow and Activity types.

Important constraints:

- a Worker Entity is associated with one task queue;
- all workers polling the same task queue must register the same Workflow and
  Activity types;
- if a Worker receives a task for an unknown type, that task fails.

2607 implication:

- start with three local task queues: `symphony-core`, `symphony-agent`, and
  `symphony-local`;
- route `IssueWorkflow`, tracker transitions, PR-link mutation, and workflow
  control Activities to `symphony-core`;
- route Codex Main/Rework/Merge runs and heavy Agent Review backend work to
  `symphony-agent`;
- route SQLite projection, artifact indexing, tracker cache refresh, and local
  health/admin/rebuild work to `symphony-local`;
- use Activity-level concurrency limits within each queue;
- App initialization must verify that required workers are polling the expected
  queues.

## Client Constraints

Starting a Workflow requires:

- Workflow type;
- Workflow input;
- task queue;
- Workflow ID.

In 2607, `IssueWorkflow` executions are tracker-state-triggered pulses, not
one long-lived Workflow per issue. Workflow ID should therefore identify the
execution pulse, while still carrying the issue identity.

2607 implication:

- use the Coordinator-owned, human-readable, episode-scoped ID
  `issue:<encoded-host>/<encoded-owner>/<encoded-repo>:<issue-number>:pulse:<from-state>-to-<target-kind>:<YYYYMMDDTHHMMSSZ>:<source-kind>-<encoded-source-ref>`;
- use Temporal's returned `run_id` for exact Temporal execution lookup;
- Coordinator owns Workflow ID construction and enforces Shea's 256-byte
  encoded limit;
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
- Revisit persistence for a later user-facing local runtime. The T2607-01
  smoke deliberately uses non-persistent dev storage through its one supported
  harness path.
- Verify episode history-size management for active `IssueWorkflow`
  executions. Static lanes such as `Backlog` and `Human Review` should not keep
  long-lived idle Workflow executions open by default.
