# Task Queues

Status: Draft

## Purpose

Define the 2607 Temporal task queue topology for active workflow episodes and
Activities.

Temporal task queues should be used for worker capacity, isolation, and
routing. They are not merely abstract architecture layers.

## Starting Topology

2607 should start with three task queues:

```text
symphony-core
symphony-agent
symphony-local
```

Do not start with a single queue. Shea has long-running Coding Agent work that
can block short control-plane work if everything shares one queue.

Do not split into many more queues until measurement proves contention or
isolation needs.

## symphony-core

Purpose:

- ordered workflow control;
- tracker-facing facts;
- latency-sensitive control-plane work.

Owns:

- `IssueWorkflow`;
- `TrackerTransitionActivity`;
- PR-to-issue link mutation;
- workflow control Activities;
- operator action validation/update handling.

Concurrency:

- low/medium concurrency;
- external fact-changing operations remain serial per issue;
- tracker mutation concurrency should be bounded globally enough to protect
  GitHub/Project rate limits.

## symphony-agent

Purpose:

- long-running, resource-heavy agent work.

Owns:

- Codex Main agent runs;
- Rework agent runs;
- Merge agent runs;
- heavy Agent Review backend work when applicable.

Concurrency:

- very low concurrency by default;
- Activity heartbeats are required for long-running work;
- attempts produce summaries and artifact refs, not per-model-turn Workflow
  events.

## symphony-local

Purpose:

- short local work and read-model maintenance.

Owns:

- SQLite projection;
- artifact indexing;
- tracker cache refresh;
- PR summary cache refresh;
- local health checks;
- DB migration/rebuild/compact;
- local admin/read-model maintenance.

Concurrency:

- higher concurrency than `symphony-core` and `symphony-agent`;
- SQLite writes use short retryable transactions;
- projection failures mark freshness stale/failed and do not change Workflow
  truth.

## Routing Rules

Route by capacity and side-effect profile:

- latency-sensitive workflow and tracker mutations: `symphony-core`;
- long-running Coding Agent or heavy review work: `symphony-agent`;
- short local cache/index/projection/admin work: `symphony-local`.

The goal is to prevent long-running Coding Agent Activities from delaying:

- tracker transitions;
- PR link verification;
- operator action validation;
- refresh/cache projection;
- local dashboard read-model maintenance.

Task queue concurrency controls active work, not the number of static issues in
the tracker. Static lanes such as `Backlog` and `Human Review` do not consume
worker capacity unless an executable episode is started.

## Activity-Level Limits

Use Activity-level concurrency limits within each queue.

Initial configurable defaults:

```text
symphony-core:
  max_concurrent_activities: 3
  tracker_fact_write_per_issue: 1
  pr_link_mutation_per_issue_pr: 1

symphony-agent:
  max_concurrent_agent_runs: 3
  max_agent_runs_per_issue: 1
  merge_land_per_issue: 1

symphony-local:
  max_concurrent_local_activities: 8
  sqlite_write_transactions: short_retryable
```

`symphony-core` and `symphony-agent` start at 3 because previous Codex
app-server and agy CLI dogfood runs have shown that three concurrent agent-side
operations are realistic. These are starting caps, not correctness assumptions.

Examples:

- serialize merge/land per issue;
- avoid duplicate PR-link mutations for the same issue/PR pair;
- bound GitHub Project write concurrency;
- keep Codex agent attempts low concurrency but allow up to three concurrent
  agent runs by default;
- allow artifact indexing and read-only cache refresh to run with higher
  concurrency.

If GitHub Project rate limits, lock contention, or local resource pressure
appear, lower the relevant sub-limit before changing the task queue topology.

## App Initialization

App/runtime initialization should verify:

- Temporal local service is reachable;
- `symphony-core` worker is polling;
- `symphony-agent` worker is polling or intentionally disabled;
- `symphony-local` worker is polling;
- required Workflow and Activity types are registered on the expected queues.
- Workflow Coordinator/App start logic will only start executions for
  executable tracker states, up to configured capacity.

If `symphony-agent` is unavailable, the App may still show dashboard state and
allow local/admin operations, but issue implementation work should be marked
unavailable.

If `symphony-core` is unavailable, product workflow operations are unavailable.

## Non-Goals

- No one-queue bottleneck as the 2607 default.
- No large fanout of task queues before measurement.
- No custom scheduler beside Temporal.
- No queue split that gives App, CLI, or extensions direct tracker write
  authority.
