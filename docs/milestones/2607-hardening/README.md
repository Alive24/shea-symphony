# 2607 Hardening

Status: Draft

## Purpose

2607 Hardening is the post-MVP milestone for turning Shea Symphony from a
working dogfood harness into a maintainable local Temporal workflow runtime.

The MVP proved that the system can move real issues through implementation,
review, merge, and operator follow-up. This milestone focuses on hardening the
runtime boundaries before adding more product surface area.

Workflow Graph implementation is intentionally deferred to
`docs/milestones/2608-workflow-graph-extension/`. 2607 should prepare the
structure and boundaries while replacing the hand-rolled orchestration loop
with Temporal.

## MVP Baseline

The baseline is broad, not repo-specific:

- the protected 2606 MVP branch exists as the working baseline;
- Shea can run complete issue workflows against its own project;
- Shea can run complete workflows while developing other projects through a
  vendored runtime, even though that distribution model is not ideal;
- occasional human doctor/operator repair is acceptable in the MVP, but should
  become clearer and less frequent.

## Milestone Goal

Separate the reliable runtime named `Symphony` from the extension layer named
`Shea`, while preserving the workflows that already work.

Symphony should own hard execution concerns through Temporal workflows and
activities: tracker reads and writes, workflow state, worktrees, agent running,
review, merge, runtime state, logging, traceability, and status snapshots.
SQLite backs the local read model/cache/index for dashboard and artifact lookup;
it does not own workflow progression.

Shea should own extension concerns: skills, prompt templates, semantic gates,
Issue Forge, Dream/Reflect style backlog mining, and operator interaction.

## Subtraction First

2607 Hardening starts with subtraction. Do not add new user-visible product
capability until the existing runtime boundaries are clearer, repeated
control-plane work is reduced, and tracker/workflow state ownership is
explicit.

Subtraction means:

- remove the hand-rolled durable loop in favor of Temporal local workflows;
- remove duplicate tracker reads inside one workflow step;
- remove scattered tracker writes from lane-specific code;
- remove lane-local state mapping;
- remove App-owned source-of-truth interpretation;
- remove vendored runtime assumptions from target repositories;
- remove hidden state transitions not represented in tracker state, runtime
  state, or workflow evidence;
- remove direct LLM authority over hard runtime decisions.

Subtraction does not mean:

- removing working Main, Review, or Merge flows;
- removing human-in-the-loop states;
- removing Shea extension capabilities;
- rewriting the system from scratch.

## Success Criteria

- Symphony has one documented owner for tracker writes.
- Temporal local workflow is the runtime spine for issue orchestration.
- Workflow behavior has a clear state-grouped structure that can evolve toward
  a persistent Workflow Graph without breaking MVP behavior.
- Standard tracker states are explicit and resumable.
- Hooks/extensions can be inserted without gaining direct tracker write access.
- App surfaces consume status and workflow snapshots instead of owning source
  of truth state.
- Workspace and config layout no longer require vendoring the Symphony binary
  into target repositories.
- Non-LLM paths have visible performance expectations and measurement points.

## Non-Goals

- Rewriting the working MVP.
- Reimplementing the Elixir reference one-to-one.
- Adding new user-visible product capability during the subtraction phase.
- Depending on Temporal Cloud.
- Keeping the old autopilot/tick loop as a second durable runtime.
- Creating GitHub issues from every backlog note in this milestone directory.
- Building a full visual Workflow Graph editor.
- Implementing the full Workflow Graph runtime or extension module loader.
- Letting LLM nodes perform unbounded side effects.

## Workstreams

- Boundary definition: decide what belongs to Symphony and what belongs to Shea.
- Temporal spine: migrate issue orchestration to local Temporal workflows and
  activities.
- Workflow structure: define state grouping, standard behavior configuration,
  extension insertion points, and future Workflow Graph vocabulary.
- Tracker ownership: route writes through Symphony.
- Tracker transitions: route state writes through `TrackerTransitionActivity`.
- Workspace/config layout: replace vendored runtime assumptions.
- App/CLI split: make App the primary operation surface and CLI an admin/dev
  fallback.
- Snapshot/dashboard: keep top-level App refresh light and move issue details
  behind drill-down.
- Local state DB: use SQLite as a rebuildable dashboard read model, tracker
  cache, PR summary cache, and artifact index.
- Agent Activity contract: keep coding/review/merge/doctor agent work as
  coarse attempt boundaries with typed inputs, outputs, capabilities,
  heartbeat layers, and artifact refs.
- Child Workflow policy: keep `IssueWorkflow` plus coarse Activities as the
  default; promote subflows to Child Workflows only when they need independent
  durable orchestration.
- Performance: measure and reduce repeated non-LLM control-plane work.
- Implementation backlog: split the hardening work into executable packages
  without prematurely creating GitHub issues.

## Source Documents

- `design/context/local-code/shea-symphony/files/docs/bootstrap/references/openai-symphony/SPEC.md`
- `docs/bootstrap/SHEA_SYMPHONY_SPEC.md`
- `docs/codex-app-server-transport.md`
- `docs/dogfood-readiness.md`
- `docs/main-orchestration-spine.md`
- `AGENT-ACTIVITY-CONTRACT.md`
- `CHILD-WORKFLOW-POLICY.md`
- `IMPLEMENTATION-BACKLOG.md`
- `implementation/T2607-01-temporal-runtime-skeleton.md`
- `docs/milestones/2607-hardening/RUNTIME-ROLE-MAPPING.md`
- `docs/milestones/2607-hardening/TEMPORAL-RUST-SDK-INTAKE.md`
- `docs/milestones/2607-hardening/ACTIVITY-ERROR-TAXONOMY.md`
- `docs/milestones/2607-hardening/ISSUE-WORKFLOW.md`
- `docs/milestones/2607-hardening/ISSUE-WORKFLOW-STATE.md`
- `docs/milestones/2607-hardening/LOCAL-STATE-DB.md`
- `docs/milestones/2607-hardening/OPERATOR-ACTION-BRIDGE.md`
- `docs/milestones/2607-hardening/TASK-QUEUES.md`
- `docs/milestones/2607-hardening/TEMPORAL-CONCURRENCY.md`
- `docs/milestones/2607-hardening/TEMPORAL-SPINE.md`
- `docs/milestones/2607-hardening/WORKFLOW-ACTIVATION.md`
- `docs/milestones/2607-hardening/SUBTRACTION-INVENTORY.md`
- `docs/milestones/2607-hardening/SNAPSHOT-AND-DASHBOARD.md`
- `docs/milestones/2607-hardening/TRACKER-TRANSITIONS.md`
- `docs/milestones/2607-hardening/TRACKER-TRANSITION-ACTIVITY.md`
- `docs/milestones/2607-hardening/adr/0006-temporal-local-runtime-spine.md`
- `docs/milestones/2607-hardening/adr/0007-local-state-db-read-model.md`
- `docs/milestones/2608-workflow-graph-extension/README.md`

## Open Questions

See `QUESTIONS.md`.

## Decisions

Decision records live under `adr/`.

## Backlog Notes

Backlog notes live under `backlog/`. They are idea capture only until promoted
through the normal issue workflow.
