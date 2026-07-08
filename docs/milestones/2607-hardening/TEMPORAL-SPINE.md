# Temporal Spine

Status: Draft

## Purpose

2607 Hardening should migrate Symphony's hand-rolled orchestration loop to a
local Temporal runtime.

This is not a spike and not an adapter for later. Temporal becomes the
Symphony runtime spine for durable workflow state, retries, waiting, signals,
queries, activity history, and cancellation.

The protected 2606 MVP branch is the fallback. 2607 does not need to preserve
the old loop as a second durable runtime.

## Hard Decision

Use Temporal local-first:

- local Temporal service is the only orchestration backend for 2607;
- Temporal Cloud is out of scope;
- Temporal Workflow owns durable issue orchestration;
- old autopilot/tick/resume loop is legacy-to-delete;
- all side effects run through Activities;
- App operations use Temporal start/query/signal/update through the Tauri
  backend command layer;
- CLI is admin/dev fallback only.

Do not introduce an independent local Symphony service in 2607. The Tauri
backend command layer is enough for the App.

Use `symphony` as the core runtime naming boundary. Temporal is the runtime
spine inside Symphony, not a reason to introduce a separate
`temporal_runtime` package name by default.

## SDK Grounding

Implementation contracts must follow `TEMPORAL-RUST-SDK-INTAKE.md`.

Hard constraints from the Rust SDK intake:

- Workflow code is deterministic orchestration only;
- side effects are Activities;
- large payloads stay out of Workflow history;
- dashboard reads use Queries;
- operator actions use Signals or Updates;
- one local task queue is enough until measured otherwise.

## Runtime Shape

```text
Tauri App
  -> Tauri Rust backend commands
      -> Temporal Client
      -> local config/artifact/workspace stores

Temporal local service
  -> local durable workflow history

Symphony Worker
  -> Temporal Worker
      -> IssueWorkflow
      -> Activities for tracker, git, worktree, Codex, LLM, review, merge,
         doctor, and artifact writes

CLI
  -> optional admin/dev fallback only
```

## IssueWorkflow

`IssueWorkflow` should cover every standard Shea Symphony state from the start:

- `Backlog`
- `Todo`
- `Need to Clarify`
- `In Progress`
- `Need Human Input`
- `Agent Review`
- `Human Review`
- `Rework`
- `Merging`
- `Done`

Do not create a temporary reduced state machine that omits `Backlog`,
`Agent Review`, `Rework`, `Need Human Input`, or `Need to Clarify`. Those
states are core to Shea Symphony.

Backlog promotion and quality gates belong inside `IssueWorkflow`. They should
not remain external pre-work.

## Activities

Side effects belong in Activities:

- `ContractCheckActivity`
- `BacklogQualityGateActivity`
- `TrackerTransitionActivity`
- `MainAgentActivity`
- `AgentReviewActivity`
- `HumanReviewValidationActivity`
- `ReworkActivity`
- `MergeActivity`
- `MergeSemanticFixActivity`, if semantic fix is not folded into
  `MergeActivity`
- `DoctorActivity`
- `WorktreeActivity`
- `ArtifactWriteActivity`

Workflow code should orchestrate. Activities should perform I/O.

Use `RUNTIME-ROLE-MAPPING.md` to keep Activity boundaries repo-grounded:

- Codex app-server remains the coding runtime behind coarse implementation or
  rework Activities;
- review backends remain review Activities that return typed verdicts;
- Shea skills and prompts shape task context and evidence, not workflow
  durability;
- Rig, MCP, and vector RAG are deferred unless a later milestone proves they
  are needed.

## Signals And Updates

Operator actions should enter the workflow through Temporal signals or updates:

- submit clarification;
- promote backlog item to todo;
- submit backlog context;
- submit human input;
- approve human review;
- request rework;
- pause;
- resume;
- cancel.

Use updates when the caller needs synchronous accepted/rejected feedback. Use
signals for fire-and-continue actions. If Rust SDK support makes updates
awkward, use signal plus query first without changing the architecture.

## Queries

Dashboard and detail reads should be Temporal query-backed:

- dashboard snapshot;
- issue detail;
- current state;
- waiting reason;
- active PR summary;
- artifact references.

The App should not run workflow internals to refresh. It should query current
workflow state and lazy-load artifact details.

## App And CLI

The App is the primary operation surface.

First-run and local runtime setup should be App-first:

- initialize `~/.shea`;
- verify local Temporal service;
- start or connect local Temporal service;
- start or connect Symphony worker;
- select canonical repository;
- verify GitHub auth;
- verify repo and workspace config.

CLI may keep only admin/dev fallback commands:

- `shea init`;
- `shea doctor-local`;
- `shea worker run`;
- commands needed for CI or App recovery.

CLI must not keep workflow product semantics such as tick, autopilot, merge,
review, or doctor-as-mutator. Those are Temporal workflow operations,
activities, queries, signals, or updates.

## Local Traceability

Temporal local history is the primary ordering and retry trace:

- workflow events;
- activity lifecycle;
- retries;
- timers;
- signals;
- state transition summaries;
- artifact references.

Large payloads stay outside Temporal history:

```text
~/.shea/artifacts/<workflow-id>/
  agent-transcript/
  logs/
  reports/
  patches/
  screenshots/
```

Temporal history should store ids, summaries, and artifact refs, not full
transcripts or large reports.

## Tracker State

Tracker state remains the external workflow fact. Temporal is the durable
execution spine.

Tracker writes happen through `TrackerTransitionActivity`, which:

- validates the requested transition;
- writes tracker state;
- writes evidence or artifact references;
- returns the committed transition summary to the workflow.

No lane, extension, App command, or CLI command writes tracker state directly.

## Deletion Target

The old Symphony loop should end as:

- deleted; or
- reduced to compatibility shims that start/query/signal Temporal workflows.

It should not remain a second retry/resume/state/history framework.
